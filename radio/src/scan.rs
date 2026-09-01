//! Turning a BSS scan into observations.
//!
//! On the Orbic RC400L this is the only Wi-Fi capture method available: the
//! QCA9377's qcacld-2.0 driver compiles monitor mode in but never advertises
//! `NL80211_IFTYPE_MONITOR`, and no `con_mode` value exposes it (see
//! `doc/radio-capability-rc400l.md`). BSS scanning still works well and, on a
//! dedicated managed interface, runs without disturbing the device's own
//! access point.
//!
//! What that costs is stated plainly here because it decides which signatures
//! can ever fire: a BSS scan sees **beacons and probe responses from access
//! points**. It does not see probe requests, so a device that only ever
//! transmits probe requests — which is how current Flock cameras behave — is
//! invisible to this backend no matter how good the signature is.

use crate::mac::MacAddr;
use crate::observation::{FrameKind, InformationElement, Security, Ssid, WifiObservation};

/// A source of Wi-Fi observations.
///
/// Implemented by the `iw`-backed scanner today; an nl80211 backend that
/// returns raw information elements, or a companion radio feeding frames in
/// over IPC, can be substituted without touching the analysis engine.
pub trait WifiScanner {
    type Error;
    fn scan(&mut self) -> Result<Vec<WifiObservation>, Self::Error>;
}

/// Elements `iw` prints by name, mapped back to their IEEE element IDs.
///
/// `iw` decodes elements rather than emitting their bytes, so this recovers
/// only which elements were *present*. It deliberately does not synthesise
/// payloads: an [`InformationElement`] produced here has an empty `data`, and
/// signatures needing element contents will correctly fail to match rather
/// than match against invented bytes.
const PRINTED_ELEMENT_NAMES: &[(&str, u8)] = &[
    ("SSID:", 0),
    ("Supported rates:", 1),
    ("DS Parameter set:", 3),
    ("TIM:", 5),
    ("Country:", 7),
    ("ERP:", 42),
    ("RSN:", 48),
    ("Extended supported rates:", 50),
    ("HT capabilities:", 45),
    ("HT operation:", 61),
    ("Extended capabilities:", 127),
    ("VHT capabilities:", 191),
    ("VHT operation:", 192),
];

/// Undo the `\xNN` escaping `iw` applies to SSID bytes it will not print.
///
/// `iw` escapes every non-printable and non-ASCII byte, so an SSID containing
/// anything outside plain ASCII arrives as literal backslash-x text. Left
/// alone, a network named with a curly apostrophe shows up in the UI as
/// `Jan\xe2\x80\x99s House` rather than `Jan's House`, and any SSID rule
/// written against the real name silently fails to match. Decoding here means
/// the rest of the crate only ever sees real bytes.
fn unescape_iw(text: &str) -> Vec<u8> {
    let raw = text.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        // \xNN -> one byte
        if raw[i] == b'\\' && i + 3 < raw.len() && raw[i + 1] == b'x' {
            let hi = (raw[i + 2] as char).to_digit(16);
            let lo = (raw[i + 3] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 4;
                continue;
            }
        }
        // \\ -> a single backslash, so a name really containing one survives.
        if raw[i] == b'\\' && i + 1 < raw.len() && raw[i + 1] == b'\\' {
            out.push(b'\\');
            i += 2;
            continue;
        }
        out.push(raw[i]);
        i += 1;
    }
    out
}

/// Settle the security flags once a record is complete.
///
/// The capability line appears before the RSN/WPA blocks, so "Privacy is set"
/// is known first and "which scheme" only later. A network with Privacy set
/// and neither block advertised is WEP, which can only be concluded once the
/// whole record has been read.
fn finish_security(obs: &mut WifiObservation) {
    if let Some(sec) = obs.security.as_mut()
        && !sec.open
        && !sec.wpa
        && !sec.wpa2
        && !sec.wpa3
    {
        sec.wep = true;
    }
}

/// Parse the output of `iw dev <iface> scan`.
///
/// Written as a pure function over text so it can be tested against captured
/// device output without hardware. Unparseable lines are skipped rather than
/// treated as fatal: this input is ultimately attacker-influenced, since a
/// hostile transmitter chooses its own SSID and elements.
pub fn parse_iw_scan(output: &str) -> Vec<WifiObservation> {
    let mut out = Vec::new();
    let mut current: Option<WifiObservation> = None;

    for line in output.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("BSS ") {
            if let Some(mut done) = current.take() {
                finish_security(&mut done);
                out.push(done);
            }
            let mut obs = WifiObservation::empty();
            // "BSS aa:bb:cc:dd:ee:ff(on scan0) -- associated"
            let addr_text = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .split('(')
                .next()
                .unwrap_or("");
            if let Ok(mac) = MacAddr::parse(addr_text) {
                obs.bssid = Some(mac);
                // A scan result is a beacon or a probe response; the two are
                // not distinguishable here, and both are transmitted by the AP.
                obs.transmitter = Some(mac);
            }
            obs.frame = Some(FrameKind::BeaconOrProbeResponse);
            current = Some(obs);
            continue;
        }

        let Some(obs) = current.as_mut() else {
            continue;
        };

        if let Some(v) = trimmed.strip_prefix("freq:") {
            if let Ok(freq) = v.trim().parse::<f64>() {
                obs.frequency_mhz = Some(freq as u32);
            }
        } else if let Some(v) = trimmed.strip_prefix("signal:") {
            // "-27.00 dBm"
            if let Some(num) = v.split_whitespace().next()
                && let Ok(dbm) = num.parse::<f64>()
            {
                obs.rssi_dbm = Some(dbm.round() as i16);
            }
        } else if let Some(v) = trimmed.strip_prefix("capability:") {
            // The Privacy bit is the only reliable "is it encrypted at all"
            // signal. Which scheme is used comes from the RSN/WPA blocks below,
            // which appear later in the same record.
            let sec = obs.security.get_or_insert_with(Security::default);
            if !v.contains("Privacy") {
                sec.open = true;
            }
        } else if trimmed.starts_with("RSN:") {
            let sec = obs.security.get_or_insert_with(Security::default);
            sec.wpa2 = true;
            sec.open = false;
        } else if trimmed.starts_with("WPA:") {
            let sec = obs.security.get_or_insert_with(Security::default);
            sec.wpa = true;
            sec.open = false;
        } else if trimmed.starts_with("WPS:") {
            obs.wps = true;
        } else if let Some(v) = trimmed.strip_prefix("* Authentication suites:") {
            let sec = obs.security.get_or_insert_with(Security::default);
            // 00-0f-ac:8 is SAE, i.e. WPA3. iw prints the suite OUI rather
            // than a name for anything it does not recognise.
            if v.contains("00-0f-ac:8") || v.contains("SAE") {
                sec.wpa3 = true;
            }
            if v.contains("802.1X") {
                sec.enterprise = true;
            }
        } else if let Some(v) = trimmed.strip_prefix("last seen:") {
            if let Some(num) = v.split_whitespace().next()
                && let Ok(ms) = num.parse::<u32>()
            {
                obs.last_seen_ms = Some(ms);
            }
        } else if let Some(v) = trimmed.strip_prefix("SSID:") {
            // A zero-length SSID in a beacon is a hidden network. It is stored
            // as the same wildcard variant a probe request would use; the
            // frame type is what distinguishes the two cases.
            let raw = v.strip_prefix(' ').unwrap_or(v);
            obs.ssid = Some(Ssid::from_bytes(&unescape_iw(raw)));
        }

        for (name, id) in PRINTED_ELEMENT_NAMES {
            if trimmed.starts_with(name) && !obs.information_elements.iter().any(|ie| ie.id == *id)
            {
                obs.information_elements
                    .push(InformationElement::new(*id, &[]));
            }
        }
    }

    if let Some(mut done) = current.take() {
        finish_security(&mut done);
        out.push(done);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from an Orbic RC400L (QCA9377) running
    /// `iw dev scan0 scan` on a managed interface alongside the live AP.
    const REAL_SCAN: &str = r#"BSS 3c:52:a1:fe:c9:8b(on scan0)
	TSF: 596787826 usec (0d, 00:09:56)
	freq: 2417
	beacon interval: 100
	capability: ESS Privacy ShortPreamble ShortSlotTime (0x1431)
	signal: -27.00 dBm
	last seen: 0 ms ago
	SSID: Fios-X42QE
	Supported rates: 1.0* 2.0* 5.5* 11.0* 6.0 9.0 12.0 18.0
	DS Parameter set: channel 2
	Country: US	Environment: Indoor/Outdoor
	ERP: <no flags>
	Extended supported rates: 24.0 36.0 48.0 54.0
	RSN:	 * Version: 1
	HT capabilities:
		Capabilities: 0x9ad
BSS 70:b3:d5:7c:b4:01(on scan0)
	freq: 5745
	signal: -71.00 dBm
	SSID:
	HT capabilities:
"#;

    #[test]
    fn parses_captured_device_output() {
        let networks = parse_iw_scan(REAL_SCAN);
        assert_eq!(networks.len(), 2);

        let first = &networks[0];
        assert_eq!(
            first.bssid,
            Some(MacAddr::parse("3c:52:a1:fe:c9:8b").unwrap())
        );
        assert_eq!(first.frequency_mhz, Some(2417));
        assert_eq!(first.rssi_dbm, Some(-27));
        assert_eq!(first.ssid.as_ref().unwrap().display(), "Fios-X42QE");
        assert_eq!(first.frame, Some(FrameKind::BeaconOrProbeResponse));
        assert_eq!(first.channel(), Some(2));
    }

    #[test]
    fn a_scan_result_attributes_the_frame_to_the_access_point() {
        let networks = parse_iw_scan(REAL_SCAN);
        // The AP transmitted the beacon, so BSSID and transmitter agree. This
        // is what lets an addr2 signature match a scan result at all.
        assert_eq!(networks[0].bssid, networks[0].transmitter);
    }

    #[test]
    fn hidden_network_yields_a_zero_length_ssid() {
        let networks = parse_iw_scan(REAL_SCAN);
        let hidden = &networks[1];
        assert_eq!(hidden.ssid, Some(Ssid::Wildcard));
        assert_eq!(hidden.frequency_mhz, Some(5745));
        assert_eq!(hidden.channel(), Some(149));
        assert_eq!(hidden.rssi_dbm, Some(-71));
    }

    #[test]
    fn records_which_elements_were_present_without_inventing_payloads() {
        let networks = parse_iw_scan(REAL_SCAN);
        let ids: Vec<u8> = networks[0]
            .information_elements
            .iter()
            .map(|ie| ie.id)
            .collect();
        assert!(ids.contains(&0)); // SSID
        assert!(ids.contains(&48)); // RSN
        assert!(ids.contains(&45)); // HT capabilities
        // Payloads are not fabricated from iw's decoded text.
        assert!(
            networks[0]
                .information_elements
                .iter()
                .all(|ie| ie.data.is_empty())
        );
    }

    /// Seen on a real device: iw escapes every non-ASCII byte, so a network
    /// named with a curly apostrophe arrives as literal backslash-x text. Left
    /// undecoded it displays as gibberish and no SSID rule against the real
    /// name can ever match.
    #[test]
    fn non_ascii_ssids_are_decoded_rather_than_shown_as_escapes() {
        let scan =
            "BSS 7e:90:bc:b7:36:0d(on wlan0)\n\tfreq: 2412\n\tSSID: Jan\\xe2\\x80\\x99s House\n";
        let networks = parse_iw_scan(scan);
        assert_eq!(
            networks[0].ssid.as_ref().unwrap().display(),
            "Jan\u{2019}s House"
        );
    }

    #[test]
    fn a_literal_backslash_in_an_ssid_survives() {
        let scan = "BSS aa:bb:cc:dd:ee:01(on wlan0)\n\tSSID: back\\\\slash\n";
        let networks = parse_iw_scan(scan);
        assert_eq!(networks[0].ssid.as_ref().unwrap().display(), "back\\slash");
    }

    #[test]
    fn a_malformed_escape_is_left_alone_rather_than_dropped() {
        let scan = "BSS aa:bb:cc:dd:ee:02(on wlan0)\n\tSSID: bad\\xZZ\n";
        let networks = parse_iw_scan(scan);
        assert_eq!(networks[0].ssid.as_ref().unwrap().display(), "bad\\xZZ");
    }

    /// Shapes taken from a real scan on the device: an open network, WPA2
    /// with a pre-shared key, a WPA2/WPA3 transition network (SAE shows up as
    /// the suite OUI 00-0f-ac:8), enterprise, and WEP.
    const SECURITY_SCAN: &str = r#"BSS aa:bb:cc:00:00:01(on wlan0)
	capability: ESS (0x1001)
	signal: -50.00 dBm
	last seen: 120 ms ago
	SSID: OpenNet
BSS aa:bb:cc:00:00:02(on wlan0)
	capability: ESS Privacy ShortSlotTime (0x1411)
	SSID: HomeWPA2
	RSN:	 * Version: 1
		 * Authentication suites: PSK
BSS aa:bb:cc:00:00:03(on wlan0)
	capability: ESS Privacy (0x1011)
	SSID: Transition
	RSN:	 * Version: 1
		 * Authentication suites: PSK 00-0f-ac:8
BSS aa:bb:cc:00:00:04(on wlan0)
	capability: ESS Privacy (0x1011)
	SSID: CorpNet
	RSN:	 * Version: 1
		 * Authentication suites: IEEE 802.1X
BSS aa:bb:cc:00:00:05(on wlan0)
	capability: ESS Privacy (0x1011)
	SSID: OldWep
BSS aa:bb:cc:00:00:06(on wlan0)
	capability: ESS Privacy (0x1011)
	SSID: HasWps
	WPS:	 * Version: 1.0
	RSN:	 * Version: 1
		 * Authentication suites: PSK
"#;

    #[test]
    fn security_is_derived_from_capability_and_rsn() {
        let nets = parse_iw_scan(SECURITY_SCAN);
        assert_eq!(nets.len(), 6);
        let sec = |i: usize| nets[i].security.expect("security parsed");

        assert!(sec(0).open);
        assert_eq!(sec(0).label(), "Open");
        assert!(sec(0).is_unprotected());

        assert!(sec(1).wpa2 && !sec(1).wpa3);
        assert_eq!(sec(1).label(), "WPA2");
        assert!(!sec(1).is_unprotected());

        // Transition mode advertises both PSK and SAE.
        assert!(sec(2).wpa2 && sec(2).wpa3);
        assert_eq!(sec(2).label(), "WPA2/WPA3");

        assert!(sec(3).enterprise);
        assert_eq!(sec(3).label(), "WPA2 Enterprise");

        // Privacy set but no RSN or WPA block advertised: WEP.
        assert!(sec(4).wep);
        assert_eq!(sec(4).label(), "WEP");
        assert!(sec(4).is_unprotected());
    }

    #[test]
    fn wps_and_last_seen_are_captured() {
        let nets = parse_iw_scan(SECURITY_SCAN);
        assert!(!nets[0].wps);
        assert!(nets[5].wps);
        assert_eq!(nets[0].last_seen_ms, Some(120));
        assert_eq!(nets[1].last_seen_ms, None);
    }

    #[test]
    fn empty_input_yields_no_observations() {
        assert!(parse_iw_scan("").is_empty());
        assert!(parse_iw_scan("\n\n  \n").is_empty());
    }

    #[test]
    fn malformed_input_does_not_panic() {
        let junk = "BSS not-a-mac(on x)\n\tfreq: abc\n\tsignal: xx dBm\n\tSSID:\u{0}\u{1}\n";
        let parsed = parse_iw_scan(junk);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].bssid, None);
        assert_eq!(parsed[0].frequency_mhz, None);
        assert_eq!(parsed[0].rssi_dbm, None);
    }

    #[test]
    fn hostile_ssid_is_captured_but_neutralised_on_display() {
        let hostile = "BSS aa:bb:cc:dd:ee:ff(on scan0)\n\tSSID: \u{1b}[2J<script>\n";
        let parsed = parse_iw_scan(hostile);
        let shown = parsed[0].ssid.as_ref().unwrap().display();
        assert!(!shown.contains('\u{1b}'));
        // Escaping markup is the presentation layer's job; the raw text is
        // preserved so the UI can escape it correctly.
        assert!(shown.contains("<script>"));
    }

    #[test]
    fn trailing_bss_is_not_dropped() {
        let two = "BSS aa:bb:cc:dd:ee:01(on scan0)\n\tfreq: 2412\nBSS aa:bb:cc:dd:ee:02(on scan0)\n\tfreq: 2437\n";
        let parsed = parse_iw_scan(two);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].frequency_mhz, Some(2437));
    }

    #[test]
    fn lines_before_the_first_bss_are_ignored() {
        let noisy = "command failed: warning\n\tfreq: 2412\nBSS aa:bb:cc:dd:ee:01(on scan0)\n\tfreq: 2462\n";
        let parsed = parse_iw_scan(noisy);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].frequency_mhz, Some(2462));
    }
}
