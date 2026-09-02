//! What Rayhunter knew about itself and its surroundings when a recording was
//! made, saved beside the capture as `<name>-meta.json`.
//!
//! A capture on its own says what the towers said. It does not say which
//! device heard them, which carrier that device belonged to, what its clock
//! thought the time was, or whether it was short of disk or memory at the
//! time. Analysis needs some of that (the home network, to tell a foreign
//! tower from one's own), and anyone reading a recording later needs the rest
//! to judge it. This sidecar records it once, at the start, and adds the clock
//! readings again when the recording closes.
//!
//! The file lives in the shared library so that the standalone checker can
//! read one when it sits beside a capture, and so the daemon and the checker
//! agree on its shape.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::DeviceMetadata;

/// Bumped whenever a field changes meaning. New fields default when missing,
/// so an older reader can still open a newer file and vice versa.
pub const SIDECAR_VERSION: u32 = 1;

/// The suffix the sidecar carries after the recording's name.
pub const SIDECAR_SUFFIX: &str = "-meta.json";

/// Everything recorded about the device for one recording.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecordingSidecar {
    /// Format version of this file, see [`SIDECAR_VERSION`].
    pub sidecar_version: u32,
    /// The recording this describes: the manifest entry's name.
    pub recording: String,
    #[serde(default)]
    pub software: SoftwareInfo,
    #[serde(default)]
    pub hardware: HardwareInfo,
    /// The subscriber's home networks as `"MCC-MNC"`, read from the SIM when
    /// the recording started. Empty when the SIM could not be read, which
    /// analysis treats as unknown rather than as a mismatch.
    #[serde(default)]
    pub home_plmn: BTreeSet<String>,
    #[serde(default)]
    pub clock: ClockInfo,
    #[serde(default)]
    pub resources: ResourceInfo,
    /// The device's own WiFi, when the daemon manages one. `None` when it was
    /// not asked, or when this copy has been redacted.
    #[serde(default)]
    pub wifi: Option<WifiInfo>,
    /// Names of the fields removed before this copy was shared. Empty on the
    /// device itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted_fields: Vec<String>,
}

/// The Rayhunter build and the operating system it ran on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SoftwareInfo {
    #[serde(default)]
    pub rayhunter_version: String,
    #[serde(default)]
    pub system_os: String,
    #[serde(default)]
    pub arch: String,
    /// The kernel's own banner, the first line of `/proc/version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
}

/// The hotspot hardware, as far as it can be read from the running system.
/// Every field but `device` is best effort: a model that does not expose one
/// of these simply leaves it out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HardwareInfo {
    /// Rayhunter's name for the device type, as in the config file.
    #[serde(default)]
    pub device: String,
    /// The board or product name, for example a device-tree model string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The vendor's hardware revision, where the firmware states one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_version: Option<String>,
    /// The modem chipset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soc: Option<String>,
    /// The vendor firmware's build identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_build: Option<String>,
}

/// The clock readings that let a reader place the recording in time and judge
/// how far to trust its timestamps.
///
/// These devices have no battery-backed clock, so the system time can be
/// years off until something corrects it. Rayhunter keeps a correction in
/// memory ([`crate::clock`]) rather than changing the system clock, so both
/// the raw reading and the correction are saved here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClockInfo {
    /// The system clock as it stood when the recording began, uncorrected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_time_at_start: Option<DateTime<Local>>,
    /// The seconds Rayhunter was adding to the system clock at that moment,
    /// from a browser or GPS time sync. Zero when nothing had set it.
    #[serde(default)]
    pub offset_seconds_at_start: i64,
    /// The system clock plus the correction: the time the recording's own
    /// timestamps are stamped in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrected_time_at_start: Option<DateTime<Local>>,
    /// Seconds since boot when the recording began. Immune to clock changes,
    /// so two recordings from one boot can be placed relative to each other
    /// even when the wall clock was wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_seconds_at_start: Option<f64>,
    /// The system clock when the recording closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_time_at_end: Option<DateTime<Local>>,
    /// The correction in force when the recording closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_seconds_at_end: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_seconds_at_end: Option<f64>,
    /// True when the correction changed while recording, which means
    /// timestamps early in the recording were stamped under a different
    /// correction than later ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_changed_during_recording: Option<bool>,
}

/// Storage and memory as the recording began.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceInfo {
    /// Where the recording was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_available_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_total_kb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_available_kb: Option<u64>,
}

/// The device's WiFi client, which matters because a hotspot joined to
/// another network can be reached, and reached into, from that network.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WifiInfo {
    /// Whether the daemon was configured to join a network at all.
    #[serde(default)]
    pub client_enabled: bool,
    /// The client's state in the daemon's own words, for example
    /// `"connected"` or `"disconnected"`.
    #[serde(default)]
    pub client_state: String,
    /// The network it was joined to, when connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected_network: Option<String>,
}

impl RecordingSidecar {
    /// The metadata analysis needs, as recorded. A sidecar with an empty home
    /// network list gives an empty set, which analysis treats as unknown.
    pub fn device_metadata(&self) -> DeviceMetadata {
        DeviceMetadata {
            home_plmn: self.home_plmn.clone(),
        }
    }

    /// The copy that goes into a bundle meant for other people.
    ///
    /// The home network narrows down who the subscriber is, and the WiFi
    /// block names the network the device was on, so both go. The rest,
    /// which build ran on what hardware and what its clock read, is what a
    /// reader needs to judge the capture and identifies no one.
    pub fn redacted(&self) -> Self {
        let mut copy = self.clone();
        let mut removed = Vec::new();
        if !copy.home_plmn.is_empty() {
            copy.home_plmn.clear();
            removed.push("home_plmn".to_string());
        }
        if copy.wifi.is_some() {
            copy.wifi = None;
            removed.push("wifi".to_string());
        }
        copy.redacted_fields.extend(removed);
        copy
    }
}

/// The sidecar's file name for a recording.
pub fn sidecar_filename(entry_name: &str) -> String {
    format!("{entry_name}{SIDECAR_SUFFIX}")
}

/// Where the sidecar would sit beside a capture file, judging by the
/// capture's name. `recording.qmdl`, `recording.qmdl.gz` and
/// `recording.pcapng` all point at `recording-meta.json` in the same
/// directory. A file whose name does not look like a capture gives `None`.
pub fn sidecar_path_beside(capture: &Path) -> Option<PathBuf> {
    let file_name = capture.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".gz").unwrap_or(file_name);
    let stem = [".qmdl", ".pcapng", ".pcap"]
        .iter()
        .find_map(|ext| stem.strip_suffix(ext))?;
    if stem.is_empty() {
        return None;
    }
    Some(capture.with_file_name(sidecar_filename(stem)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RecordingSidecar {
        RecordingSidecar {
            sidecar_version: SIDECAR_VERSION,
            recording: "1700000000-0".into(),
            home_plmn: ["311-480".to_string()].into_iter().collect(),
            wifi: Some(WifiInfo {
                client_enabled: true,
                client_state: "connected".into(),
                connected_network: Some("home".into()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn redaction_removes_identifying_blocks_and_says_so() {
        let redacted = sample().redacted();
        assert!(redacted.home_plmn.is_empty());
        assert!(redacted.wifi.is_none());
        assert_eq!(redacted.redacted_fields, vec!["home_plmn", "wifi"]);
        // Redacting twice does not claim to have removed things again.
        assert_eq!(
            redacted.redacted().redacted_fields,
            redacted.redacted_fields
        );
    }

    #[test]
    fn redaction_of_an_empty_sidecar_lists_nothing() {
        let redacted = RecordingSidecar::default().redacted();
        assert!(redacted.redacted_fields.is_empty());
    }

    #[test]
    fn round_trips_through_json_and_tolerates_missing_fields() {
        let json = serde_json::to_string(&sample()).unwrap();
        let back: RecordingSidecar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sample());

        // A file from a build that knew fewer fields still opens.
        let sparse: RecordingSidecar =
            serde_json::from_str(r#"{"sidecar_version":1,"recording":"x"}"#).unwrap();
        assert_eq!(sparse.recording, "x");
        assert!(sparse.home_plmn.is_empty());
        assert!(sparse.wifi.is_none());
        assert!(sparse.device_metadata().home_plmn.is_empty());
    }

    #[test]
    fn device_metadata_carries_the_home_network() {
        let plmn = sample().device_metadata().home_plmn;
        assert!(plmn.contains("311-480"));
    }

    #[test]
    fn sidecar_sits_beside_the_capture() {
        let cases = [
            (
                "/tmp/store/1700000000-0.qmdl",
                "/tmp/store/1700000000-0-meta.json",
            ),
            (
                "/tmp/store/1700000000-0.qmdl.gz",
                "/tmp/store/1700000000-0-meta.json",
            ),
            ("rec.pcapng", "rec-meta.json"),
            ("rec.pcap", "rec-meta.json"),
        ];
        for (capture, expected) in cases {
            assert_eq!(
                sidecar_path_beside(Path::new(capture)),
                Some(PathBuf::from(expected)),
                "{capture}"
            );
        }
        assert_eq!(sidecar_path_beside(Path::new("notes.txt")), None);
        assert_eq!(sidecar_path_beside(Path::new(".qmdl")), None);
    }
}
