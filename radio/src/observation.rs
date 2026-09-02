//! What a radio saw, independent of which radio saw it.
//!
//! The analysis engine must not care whether an observation came from the
//! Orbic's own QCA9377, from a future companion board, or from another
//! Rayhunter platform. Everything downstream of this module works on
//! [`RadioObservation`], so a hardware limitation on one device never becomes
//! an architectural limitation for the project.

use crate::mac::MacAddr;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
/// Which radio technology produced an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioTech {
    Wifi,
    Ble,
    RemoteId,
}

impl RadioTech {
    /// Short badge used by the web UI, per the wireless-surveillance UI spec.
    pub const fn badge(&self) -> &'static str {
        match self {
            RadioTech::Wifi => "WIFI",
            RadioTech::Ble => "BLE",
            RadioTech::RemoteId => "REMOTE ID",
        }
    }
}

/// Where an observation came from. Recorded so that evidence remains
/// interpretable when several sources feed one analysis engine, and so that a
/// capability gap on one source can be explained to the user rather than
/// silently looking like "nothing is out there".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    /// The host device's own Wi-Fi radio, via BSS scanning.
    HostWifiScan,
    /// The host device's own Wi-Fi radio in monitor mode.
    HostWifiMonitor,
    /// The host device's own Bluetooth controller.
    HostBle,
    /// An attached companion radio, identified by a caller-supplied name.
    External(String),
}

/// The 802.11 frame that carried an observation, when the capture method can
/// tell. BSS scanning cannot distinguish a beacon from a probe response, so it
/// reports [`FrameKind::BeaconOrProbeResponse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Beacon,
    ProbeRequest,
    ProbeResponse,
    BeaconOrProbeResponse,
    Data,
    Other,
}

/// An SSID as it appeared on the air.
///
/// A zero-length SSID element is a wildcard: in a probe request it asks every
/// nearby AP to answer, which is the behaviour Flock cameras exhibit. It is
/// meaningfully different from an absent SSID element, so the two are distinct
/// variants rather than an empty string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ssid {
    /// A zero-length SSID element (wildcard / broadcast probe).
    Wildcard,
    /// A named network. Bytes are kept as received; see [`Ssid::display`].
    Named(Vec<u8>),
}

impl Ssid {
    /// Maximum length of an SSID element, from IEEE 802.11. Longer values are
    /// malformed and are truncated on ingest rather than trusted.
    pub const MAX_LEN: usize = 32;

    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            Ssid::Wildcard
        } else {
            let end = bytes.len().min(Self::MAX_LEN);
            Ssid::Named(bytes[..end].to_vec())
        }
    }

    pub const fn is_wildcard(&self) -> bool {
        matches!(self, Ssid::Wildcard)
    }

    /// A lossy, printable rendering for matching and display.
    ///
    /// SSIDs are attacker-controlled: they are arbitrary bytes chosen by
    /// whoever is transmitting, and this scanner may be running next to
    /// hostile equipment. Control characters are replaced here so that a
    /// crafted SSID cannot smuggle escape sequences into a log or terminal.
    /// HTML escaping remains the presentation layer's job.
    pub fn display(&self) -> String {
        match self {
            Ssid::Wildcard => String::new(),
            Ssid::Named(bytes) => String::from_utf8_lossy(bytes)
                .chars()
                .map(|c| if c.is_control() { '\u{fffd}' } else { c })
                .collect(),
        }
    }
}

/// A raw 802.11 information element, kept for fingerprinting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InformationElement {
    pub id: u8,
    /// Element payload, capped at the 802.11 maximum of 255 bytes on ingest.
    pub data: Vec<u8>,
}

impl InformationElement {
    pub const MAX_DATA: usize = 255;
    /// Element ID 221: vendor-specific, whose first three bytes are an OUI.
    pub const VENDOR_SPECIFIC: u8 = 221;

    pub fn new(id: u8, data: &[u8]) -> Self {
        let end = data.len().min(Self::MAX_DATA);
        InformationElement {
            id,
            data: data[..end].to_vec(),
        }
    }

    /// The OUI of a vendor-specific element, if this is one and it is long
    /// enough to carry one.
    pub fn vendor_oui(&self) -> Option<[u8; 3]> {
        if self.id != Self::VENDOR_SPECIFIC || self.data.len() < 3 {
            return None;
        }
        Some([self.data[0], self.data[1], self.data[2]])
    }
}

/// What a network uses to protect itself, as far as a scan can tell.
///
/// Kept as independent flags rather than one enum because real networks
/// advertise combinations: a WPA2/WPA3 transition network offers both, and an
/// enterprise network is WPA2 *and* 802.1X. Collapsing that to a single value
/// would lose the distinction that matters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Security {
    /// The Privacy capability bit is clear: traffic is unencrypted.
    pub open: bool,
    /// Privacy is set but neither WPA nor RSN is advertised, which in practice
    /// means WEP.
    pub wep: bool,
    pub wpa: bool,
    pub wpa2: bool,
    /// RSN advertising SAE (`00-0f-ac:8`).
    pub wpa3: bool,
    /// 802.1X rather than a pre-shared key.
    pub enterprise: bool,
}

impl Security {
    /// A short label for display, e.g. "WPA2/WPA3" or "Open".
    pub fn label(&self) -> String {
        if self.open {
            return "Open".to_string();
        }
        let mut parts: Vec<&str> = Vec::new();
        if self.wep {
            parts.push("WEP");
        }
        if self.wpa {
            parts.push("WPA");
        }
        if self.wpa2 {
            parts.push("WPA2");
        }
        if self.wpa3 {
            parts.push("WPA3");
        }
        if parts.is_empty() {
            return "Unknown".to_string();
        }
        let mut label = parts.join("/");
        if self.enterprise {
            label.push_str(" Enterprise");
        }
        label
    }

    /// True for a network anyone can join and read traffic on. Worth
    /// surfacing: an open network, especially a hidden one, is the shape a
    /// device used for collection often takes.
    pub const fn is_unprotected(&self) -> bool {
        self.open || self.wep
    }
}

/// One Wi-Fi device or network as observed.
///
/// Every address field is optional because different capture methods populate
/// different subsets: BSS scanning yields a BSSID and nothing else, while
/// monitor-mode capture yields the three or four addresses of the frame
/// header. Matching rules state which field they apply to, so a signature
/// written for `addr2` never silently matches a BSSID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WifiObservation {
    pub bssid: Option<MacAddr>,
    /// 802.11 addr2, the transmitter.
    pub transmitter: Option<MacAddr>,
    /// 802.11 addr1, the receiver.
    pub receiver: Option<MacAddr>,
    /// 802.11 addr3, usually the BSSID in an infrastructure frame.
    pub addr3: Option<MacAddr>,
    pub ssid: Option<Ssid>,
    pub frequency_mhz: Option<u32>,
    pub rssi_dbm: Option<i16>,
    pub frame: Option<FrameKind>,
    pub information_elements: Vec<InformationElement>,
    /// How the network protects itself, where the capture method reports it.
    pub security: Option<Security>,
    /// Milliseconds since this network was last heard, as the scan reported.
    pub last_seen_ms: Option<u32>,
    /// WPS advertised. Worth showing because it is a known weak point.
    pub wps: bool,
}

impl WifiObservation {
    /// An empty observation, to be filled in by a capture backend.
    pub fn empty() -> Self {
        WifiObservation {
            bssid: None,
            transmitter: None,
            receiver: None,
            addr3: None,
            ssid: None,
            frequency_mhz: None,
            rssi_dbm: None,
            frame: None,
            information_elements: Vec::new(),
            security: None,
            last_seen_ms: None,
            wps: false,
        }
    }

    /// The address that best identifies the device, preferring the transmitter
    /// (which is the device itself) over the BSSID (which may belong to an
    /// access point merely answering it).
    pub fn primary_address(&self) -> Option<MacAddr> {
        self.transmitter.or(self.bssid).or(self.addr3)
    }

    /// Channel number derived from the centre frequency, for display.
    ///
    /// Covers the 2.4 GHz and 5 GHz bands the QCA9377 reports; returns `None`
    /// for frequencies outside them rather than guessing.
    pub fn channel(&self) -> Option<u16> {
        let freq = self.frequency_mhz?;
        match freq {
            2412..=2472 => Some(((freq - 2407) / 5) as u16),
            2484 => Some(14),
            5000..=5895 => Some(((freq - 5000) / 5) as u16),
            _ => None,
        }
    }
}

/// One BLE advertiser as observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BleObservation {
    pub address: Option<MacAddr>,
    /// True when the advertiser used a resolvable or non-resolvable private
    /// address, in which case the address is not a stable identifier.
    pub address_is_random: bool,
    pub local_name: Option<String>,
    /// Bluetooth SIG company identifier from a manufacturer-specific data field.
    pub company_id: Option<u16>,
    pub service_uuids: Vec<String>,
    pub manufacturer_data: Vec<u8>,
    pub rssi_dbm: Option<i16>,
}

/// A drone Remote ID broadcast. Kept in its own category because a compliant
/// drone nearby is not by itself evidence of surveillance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteIdObservation {
    pub uas_id: Option<String>,
    pub operator_id: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude_m: Option<f32>,
    pub speed_mps: Option<f32>,
    pub heading_deg: Option<f32>,
    pub operator_latitude: Option<f64>,
    pub operator_longitude: Option<f64>,
    pub protocol_version: Option<u8>,
    pub rssi_dbm: Option<i16>,
}

/// The payload of an observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservationPayload {
    Wifi(WifiObservation),
    Ble(BleObservation),
    RemoteId(RemoteIdObservation),
}

impl ObservationPayload {
    pub const fn tech(&self) -> RadioTech {
        match self {
            ObservationPayload::Wifi(_) => RadioTech::Wifi,
            ObservationPayload::Ble(_) => RadioTech::Ble,
            ObservationPayload::RemoteId(_) => RadioTech::RemoteId,
        }
    }

    pub fn rssi_dbm(&self) -> Option<i16> {
        match self {
            ObservationPayload::Wifi(w) => w.rssi_dbm,
            ObservationPayload::Ble(b) => b.rssi_dbm,
            ObservationPayload::RemoteId(r) => r.rssi_dbm,
        }
    }

    /// The address this observation is keyed on, if it has one.
    pub fn address(&self) -> Option<MacAddr> {
        match self {
            ObservationPayload::Wifi(w) => w.primary_address(),
            ObservationPayload::Ble(b) => b.address,
            ObservationPayload::RemoteId(_) => None,
        }
    }
}

/// A single observation, with provenance and timing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioObservation {
    pub source: ObservationSource,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    /// How many times this device has been seen since `first_seen`.
    pub observation_count: u32,
    pub payload: ObservationPayload,
}

impl RadioObservation {
    pub fn new(source: ObservationSource, at: DateTime<Utc>, payload: ObservationPayload) -> Self {
        RadioObservation {
            source,
            first_seen: at,
            last_seen: at,
            observation_count: 1,
            payload,
        }
    }

    pub fn tech(&self) -> RadioTech {
        self.payload.tech()
    }

    /// Fold a fresh sighting of the same device into this record.
    pub fn merge_sighting(&mut self, at: DateTime<Utc>) {
        if at < self.first_seen {
            self.first_seen = at;
        }
        if at > self.last_seen {
            self.last_seen = at;
        }
        self.observation_count = self.observation_count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(s: &str) -> MacAddr {
        MacAddr::parse(s).unwrap()
    }

    #[test]
    fn empty_ssid_element_is_a_wildcard_not_a_name() {
        assert_eq!(Ssid::from_bytes(b""), Ssid::Wildcard);
        assert!(Ssid::from_bytes(b"").is_wildcard());
        assert!(!Ssid::from_bytes(b"home").is_wildcard());
    }

    #[test]
    fn absent_ssid_is_distinct_from_a_wildcard() {
        let mut obs = WifiObservation::empty();
        assert_eq!(obs.ssid, None);
        obs.ssid = Some(Ssid::from_bytes(b""));
        assert_eq!(obs.ssid, Some(Ssid::Wildcard));
    }

    #[test]
    fn oversized_ssid_is_truncated_rather_than_trusted() {
        let long = vec![b'a'; 100];
        match Ssid::from_bytes(&long) {
            Ssid::Named(b) => assert_eq!(b.len(), Ssid::MAX_LEN),
            Ssid::Wildcard => panic!("expected a named SSID"),
        }
    }

    #[test]
    fn control_characters_in_an_ssid_are_neutralised() {
        // A hostile SSID trying to smuggle a terminal escape sequence.
        let hostile = Ssid::from_bytes(b"a\x1b[31mred\x07");
        let shown = hostile.display();
        assert!(!shown.contains('\x1b'));
        assert!(!shown.contains('\x07'));
        assert!(shown.contains("red"));
    }

    #[test]
    fn invalid_utf8_in_an_ssid_does_not_panic() {
        let shown = Ssid::from_bytes(&[0xff, 0xfe, b'h', b'i']).display();
        assert!(shown.contains("hi"));
    }

    #[test]
    fn oversized_information_element_is_truncated() {
        let ie = InformationElement::new(221, &vec![0u8; 400]);
        assert_eq!(ie.data.len(), InformationElement::MAX_DATA);
    }

    #[test]
    fn vendor_oui_only_returned_for_vendor_elements() {
        let vendor = InformationElement::new(221, &[0x00, 0x50, 0xf2, 0x01]);
        assert_eq!(vendor.vendor_oui(), Some([0x00, 0x50, 0xf2]));

        let ssid_element = InformationElement::new(0, &[0x00, 0x50, 0xf2]);
        assert_eq!(ssid_element.vendor_oui(), None);

        let truncated = InformationElement::new(221, &[0x00, 0x50]);
        assert_eq!(truncated.vendor_oui(), None);
    }

    #[test]
    fn transmitter_is_preferred_over_bssid_as_the_device_identity() {
        let mut obs = WifiObservation::empty();
        obs.bssid = Some(mac("aa:aa:aa:00:00:01"));
        obs.transmitter = Some(mac("bb:bb:bb:00:00:02"));
        assert_eq!(obs.primary_address(), Some(mac("bb:bb:bb:00:00:02")));

        obs.transmitter = None;
        assert_eq!(obs.primary_address(), Some(mac("aa:aa:aa:00:00:01")));
    }

    #[test]
    fn channel_is_derived_for_both_bands() {
        let mut obs = WifiObservation::empty();
        obs.frequency_mhz = Some(2412);
        assert_eq!(obs.channel(), Some(1));
        obs.frequency_mhz = Some(2437);
        assert_eq!(obs.channel(), Some(6));
        obs.frequency_mhz = Some(2484);
        assert_eq!(obs.channel(), Some(14));
        obs.frequency_mhz = Some(5180);
        assert_eq!(obs.channel(), Some(36));
        obs.frequency_mhz = Some(5745);
        assert_eq!(obs.channel(), Some(149));
        // Outside the bands this radio reports: say nothing rather than guess.
        obs.frequency_mhz = Some(60_000);
        assert_eq!(obs.channel(), None);
    }

    #[test]
    fn merging_a_sighting_widens_the_window_and_counts() {
        let t0 = DateTime::parse_from_rfc3339("2026-08-31T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let t1 = DateTime::parse_from_rfc3339("2026-08-31T23:05:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut obs = RadioObservation::new(
            ObservationSource::HostWifiScan,
            t0,
            ObservationPayload::Wifi(WifiObservation::empty()),
        );
        obs.merge_sighting(t1);
        assert_eq!(obs.first_seen, t0);
        assert_eq!(obs.last_seen, t1);
        assert_eq!(obs.observation_count, 2);

        // An out-of-order sighting moves first_seen back, not last_seen.
        let earlier = t0 - chrono::Duration::minutes(10);
        obs.merge_sighting(earlier);
        assert_eq!(obs.first_seen, earlier);
        assert_eq!(obs.last_seen, t1);
        assert_eq!(obs.observation_count, 3);
    }

    #[test]
    fn external_sources_are_indistinguishable_to_the_engine() {
        let t = Utc::now();
        let from_host = RadioObservation::new(
            ObservationSource::HostWifiScan,
            t,
            ObservationPayload::Wifi(WifiObservation::empty()),
        );
        let from_companion = RadioObservation::new(
            ObservationSource::External("esp32-s3".to_string()),
            t,
            ObservationPayload::Wifi(WifiObservation::empty()),
        );
        assert_eq!(from_host.tech(), from_companion.tech());
    }
}
