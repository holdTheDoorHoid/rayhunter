//! Durable evidence for wireless detections.
//!
//! Written to a per-recording NDJSON sidecar rather than into the QMDL. Two
//! reasons, both from the review of EFForg/rayhunter#1042: wireless
//! observations are not diag messages and forcing them into QMDL would
//! corrupt the meaning of that format, and the analysis NDJSON is rewritten
//! when a recording is re-analysed, which would erase radio alerts that no
//! cellular re-run could regenerate.
//!
//! Retention is deliberately conservative. Following-detection needs to watch
//! devices that match nothing, but that must not turn into a permanent record
//! of every person who walked past. A device that matched no signature and
//! crossed no persistence threshold is written with a salted pseudonym instead
//! of its address; see [`RetentionPolicy`].

use crate::mac::MacAddr;
use crate::observation::{ObservationSource, RadioTech};
use crate::signature::Detection;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether a record may carry a real hardware address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicy {
    /// Keep the address: this device matched a signature or crossed a
    /// persistence threshold, so it is the subject of the report.
    Identify,
    /// Replace the address with a per-session pseudonym. Used for devices that
    /// are only being counted, never accused.
    Pseudonymise,
}

/// One line of the evidence sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub timestamp: DateTime<Utc>,
    pub recording_id: String,
    pub technology: RadioTech,
    pub source: ObservationSource,
    /// The device this record is about: a MAC when retention allows it, or an
    /// opaque pseudonym when it does not.
    pub device: DeviceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_mhz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u16>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub observation_count: u32,
    /// Absent for a plain observation; present when a signature fired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection: Option<Detection>,
    /// Version of the signature pack in force, so a past alert can be
    /// explained even after the rules change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_version: Option<String>,
}

/// How a device is named in an evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "id_type", rename_all = "snake_case")]
pub enum DeviceRef {
    /// A real hardware address, retained because this device is the subject of
    /// a detection.
    Mac { mac: MacAddr },
    /// A salted, truncated hash. Stable within a session so persistence can be
    /// scored, and useless outside it.
    Pseudonym { pseudonym: String },
}

impl DeviceRef {
    /// Build a reference honouring `policy`.
    pub fn new(mac: MacAddr, policy: RetentionPolicy, salt: &[u8]) -> Self {
        match policy {
            RetentionPolicy::Identify => DeviceRef::Mac { mac },
            RetentionPolicy::Pseudonymise => DeviceRef::Pseudonym {
                pseudonym: pseudonymise(mac, salt),
            },
        }
    }

    /// Key used to correlate sightings of one device within a session.
    pub fn key(&self) -> String {
        match self {
            DeviceRef::Mac { mac } => mac.to_string(),
            DeviceRef::Pseudonym { pseudonym } => pseudonym.clone(),
        }
    }

    pub const fn is_identifying(&self) -> bool {
        matches!(self, DeviceRef::Mac { .. })
    }
}

/// Derive a session-scoped pseudonym for an address.
///
/// FNV-1a over salt then address. This is an obfuscation boundary, not a
/// security one: with a known salt the 2^48 MAC space is trivially
/// enumerable. Its job is to keep casual bystanders out of a durable log,
/// which is why the salt is generated per session and never written to disk.
fn pseudonymise(mac: MacAddr, salt: &[u8]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in salt.iter().chain(mac.octets().iter()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("anon-{hash:016x}")
}

/// Serialise records as newline-delimited JSON.
///
/// Each record is written as one line; a record that fails to serialise is
/// skipped rather than aborting the file, so one malformed observation cannot
/// cost the whole session's evidence.
pub fn to_ndjson(records: &[EvidenceRecord]) -> String {
    let mut out = String::new();
    for record in records {
        match serde_json::to_string(record) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            Err(e) => log::warn!("skipping unserialisable evidence record: {e}"),
        }
    }
    out
}

/// Parse an NDJSON sidecar, skipping lines that do not parse.
pub fn from_ndjson(input: &str) -> Vec<EvidenceRecord> {
    input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match serde_json::from_str(l) {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("skipping malformed evidence line: {e}");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::RadioTech;
    use crate::signature::{Confidence, Severity};

    fn mac(s: &str) -> MacAddr {
        MacAddr::parse(s).unwrap()
    }

    fn record(device: DeviceRef, detection: Option<Detection>) -> EvidenceRecord {
        let t = DateTime::parse_from_rfc3339("2026-08-31T23:40:00Z")
            .unwrap()
            .with_timezone(&Utc);
        EvidenceRecord {
            timestamp: t,
            recording_id: "1788231293-0".into(),
            technology: RadioTech::Wifi,
            source: ObservationSource::HostWifiScan,
            device,
            ssid: Some("Fios-X42QE".into()),
            rssi_dbm: Some(-27),
            frequency_mhz: Some(2417),
            channel: Some(2),
            first_seen: t,
            last_seen: t,
            observation_count: 1,
            detection,
            rule_version: Some("builtin-2026.08".into()),
        }
    }

    fn detection() -> Detection {
        Detection {
            signature_id: "keyw.oui".into(),
            vendor: "KeyW".into(),
            product: None,
            technology: RadioTech::Wifi,
            confidence: Confidence::Info,
            severity: Severity::Informational,
            matched_fields: vec!["BSSID matches prefix 70:b3:d5:7c:b".into()],
        }
    }

    #[test]
    fn matched_device_keeps_its_address() {
        let d = DeviceRef::new(mac("70:b3:d5:7c:b4:01"), RetentionPolicy::Identify, b"salt");
        assert!(d.is_identifying());
        assert_eq!(d.key(), "70:b3:d5:7c:b4:01");
    }

    #[test]
    fn unmatched_device_is_pseudonymised() {
        let d = DeviceRef::new(
            mac("aa:bb:cc:dd:ee:ff"),
            RetentionPolicy::Pseudonymise,
            b"salt",
        );
        assert!(!d.is_identifying());
        let json = serde_json::to_string(&d).unwrap();
        // The real address must not survive anywhere in the record.
        assert!(!json.contains("aa:bb:cc"));
        assert!(json.contains("anon-"));
    }

    #[test]
    fn pseudonym_is_stable_within_a_session_so_persistence_can_be_scored() {
        let salt = b"session-salt";
        let a = DeviceRef::new(
            mac("aa:bb:cc:dd:ee:ff"),
            RetentionPolicy::Pseudonymise,
            salt,
        );
        let b = DeviceRef::new(
            mac("aa:bb:cc:dd:ee:ff"),
            RetentionPolicy::Pseudonymise,
            salt,
        );
        assert_eq!(a.key(), b.key());
    }

    #[test]
    fn pseudonym_differs_across_sessions_and_across_devices() {
        let one = DeviceRef::new(
            mac("aa:bb:cc:dd:ee:ff"),
            RetentionPolicy::Pseudonymise,
            b"s1",
        );
        let other_session = DeviceRef::new(
            mac("aa:bb:cc:dd:ee:ff"),
            RetentionPolicy::Pseudonymise,
            b"s2",
        );
        assert_ne!(one.key(), other_session.key());

        let other_device = DeviceRef::new(
            mac("aa:bb:cc:dd:ee:01"),
            RetentionPolicy::Pseudonymise,
            b"s1",
        );
        assert_ne!(one.key(), other_device.key());
    }

    #[test]
    fn ndjson_round_trips() {
        let records = vec![
            record(
                DeviceRef::new(mac("70:b3:d5:7c:b4:01"), RetentionPolicy::Identify, b"s"),
                Some(detection()),
            ),
            record(
                DeviceRef::new(
                    mac("aa:bb:cc:dd:ee:ff"),
                    RetentionPolicy::Pseudonymise,
                    b"s",
                ),
                None,
            ),
        ];
        let text = to_ndjson(&records);
        assert_eq!(text.lines().count(), 2);
        let back = from_ndjson(&text);
        assert_eq!(back, records);
    }

    #[test]
    fn a_detection_records_why_it_fired() {
        let records = vec![record(
            DeviceRef::new(mac("70:b3:d5:7c:b4:01"), RetentionPolicy::Identify, b"s"),
            Some(detection()),
        )];
        let text = to_ndjson(&records);
        assert!(text.contains("70:b3:d5:7c:b"));
        assert!(text.contains("matched_fields"));
        // The rule version travels with the alert so it stays explicable.
        assert!(text.contains("builtin-2026.08"));
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let text = "{ not json\n{\"also\": \"wrong shape\"}\n";
        assert!(from_ndjson(text).is_empty());

        let good = to_ndjson(&[record(
            DeviceRef::new(mac("70:b3:d5:7c:b4:01"), RetentionPolicy::Identify, b"s"),
            None,
        )]);
        let mixed = format!("{{ broken\n{good}");
        assert_eq!(from_ndjson(&mixed).len(), 1);
    }
}
