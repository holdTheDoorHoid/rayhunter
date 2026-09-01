//! The `metadata.json` bundled into a downloaded recording.
//!
//! A capture on its own is hard to interpret later. The QMDL says what the
//! radio saw but not what saw it, when, on which device, or which detectors
//! had run over it and at what version. That matters most for a capture sent
//! to somebody else: EFF asking about a submission has otherwise no way to
//! know whether a quiet report means nothing happened or means a detector that
//! would have caught it had not been written yet.
//!
//! So the download carries a small sidecar answering those questions. It is
//! assembled from things Rayhunter already records, and adds nothing to the
//! capture itself: see EFForg/rayhunter#670.
//!
//! **What is deliberately not in here.** No subscriber identity, no IMSI, IMEI
//! or temporary identity, and no WiFi or account credentials. The whole point
//! is a file that is safe to attach to an email, so it must not be the thing
//! that leaks the device's identifiers.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::qmdl_store::ManifestEntry;
use rayhunter::Device;

/// Bumped when the shape of this file changes, so anything reading one knows
/// what it is looking at rather than guessing from which keys are present.
pub const METADATA_VERSION: u32 = 1;

/// What produced the recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RayhunterInfo {
    /// The daemon version that made the recording, when it was recorded.
    pub version_at_capture: Option<String>,
    /// The daemon version that assembled this download, which is not
    /// necessarily the same: a recording can be exported long after it was
    /// made, and by a newer build.
    pub version_at_export: String,
    pub system_os: Option<String>,
    pub arch: Option<String>,
    /// The configured device type, not a detected model.
    pub device: Device,
}

/// The recording itself.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordingInfo {
    pub id: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub started: DateTime<Local>,
    pub last_message: Option<DateTime<Local>>,
    pub qmdl_size_bytes: usize,
    pub stop_reason: Option<String>,
    /// Whether location was being recorded, and how.
    pub gps_mode: Option<String>,
}

/// One detector that ran over this recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalyzerInfo {
    pub name: String,
    pub version: u32,
}

/// Which detectors ran, and at what version.
///
/// The most useful part of the file for anyone reading a capture later. A
/// report with no warnings means something quite different depending on
/// whether the detector that would have caught it existed at the time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisInfo {
    pub report_version: u32,
    pub analyzers: Vec<AnalyzerInfo>,
}

/// The sidecar written into the download.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordingMetadata {
    pub metadata_version: u32,
    /// When this download was assembled, not when the recording was made.
    pub generated_at: DateTime<Local>,
    pub rayhunter: RayhunterInfo,
    pub recording: RecordingInfo,
    /// Absent when the recording has not been analysed, or the analysis file
    /// could not be read. Absent is honest; an empty analyzer list would read
    /// as "nothing ran", which is a different claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<AnalysisInfo>,
}

/// Read the detectors out of the first line of an analysis file.
///
/// The analysis file is newline delimited JSON whose first line is the report
/// metadata, so this needs the first line only rather than the whole file,
/// which on a long recording is large.
///
/// Returns `None` for anything unreadable rather than failing the download.
/// Somebody downloading a capture wants the capture; a missing sidecar section
/// is a far better outcome than no file at all.
pub fn analysis_info_from_first_line(line: &str) -> Option<AnalysisInfo> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let report_version = value.get("report_version")?.as_u64()? as u32;
    let analyzers = value
        .get("analyzers")?
        .as_array()?
        .iter()
        .filter_map(|a| {
            Some(AnalyzerInfo {
                name: a.get("name")?.as_str()?.to_string(),
                version: a.get("version")?.as_u64()? as u32,
            })
        })
        .collect();
    Some(AnalysisInfo {
        report_version,
        analyzers,
    })
}

/// Assemble the sidecar.
///
/// Pure, so the contents can be tested without building a zip: the value of
/// this file is entirely in what it says, and a field silently going missing
/// would not fail anything at run time.
pub fn build(
    entry: &ManifestEntry,
    device: &Device,
    analysis: Option<AnalysisInfo>,
    generated_at: DateTime<Local>,
) -> RecordingMetadata {
    RecordingMetadata {
        metadata_version: METADATA_VERSION,
        generated_at,
        rayhunter: RayhunterInfo {
            version_at_capture: entry.rayhunter_version.clone(),
            version_at_export: env!("CARGO_PKG_VERSION").to_string(),
            system_os: entry.system_os.clone(),
            arch: entry.arch.clone(),
            device: device.clone(),
        },
        recording: RecordingInfo {
            id: entry.name.clone(),
            display_name: entry.display_name.clone(),
            notes: entry.notes.clone(),
            started: entry.start_time,
            last_message: entry.last_message_time,
            qmdl_size_bytes: entry.qmdl_size_bytes,
            stop_reason: entry.stop_reason.clone(),
            gps_mode: entry.gps_mode.map(|mode| format!("{mode:?}")),
        },
        analysis,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ManifestEntry {
        ManifestEntry {
            name: "1788289687".to_string(),
            start_time: Local::now(),
            last_message_time: None,
            qmdl_size_bytes: 4096,
            rayhunter_version: Some("0.12.2".to_string()),
            system_os: Some("linux".to_string()),
            arch: Some("armv7".to_string()),
            stop_reason: None,
            upload_time: None,
            gps_mode: None,
            compressed: false,
            display_name: Some("market-square".to_string()),
            notes: Some("a note".to_string()),
        }
    }

    #[test]
    fn the_sidecar_carries_what_a_reader_needs() {
        let meta = build(&entry(), &Device::Orbic, None, Local::now());
        assert_eq!(meta.metadata_version, METADATA_VERSION);
        assert_eq!(meta.recording.id, "1788289687");
        assert_eq!(
            meta.recording.display_name.as_deref(),
            Some("market-square")
        );
        assert_eq!(meta.rayhunter.version_at_capture.as_deref(), Some("0.12.2"));
        assert!(!meta.rayhunter.version_at_export.is_empty());
    }

    /// A recording exported by a newer build than made it must say both
    /// versions, or somebody reading it later cannot tell which code produced
    /// the capture.
    #[test]
    fn both_versions_are_recorded_separately() {
        let mut e = entry();
        e.rayhunter_version = Some("0.1.0".to_string());
        let meta = build(&e, &Device::Orbic, None, Local::now());
        assert_eq!(meta.rayhunter.version_at_capture.as_deref(), Some("0.1.0"));
        assert_ne!(
            meta.rayhunter.version_at_capture.as_deref(),
            Some(meta.rayhunter.version_at_export.as_str())
        );
    }

    #[test]
    fn detectors_are_read_from_a_report_header() {
        let line = r#"{"analyzers":[{"name":"IMSI Requested","description":"d","version":4},{"name":"Null Cipher","description":"d","version":2}],"rayhunter":{},"report_version":3}"#;
        let info = analysis_info_from_first_line(line).expect("parses");
        assert_eq!(info.report_version, 3);
        assert_eq!(info.analyzers.len(), 2);
        assert_eq!(info.analyzers[0].name, "IMSI Requested");
        assert_eq!(info.analyzers[1].version, 2);
    }

    /// An unreadable analysis file must leave the section out rather than
    /// making the whole download fail. Somebody downloading a capture wants
    /// the capture.
    #[test]
    fn an_unreadable_report_header_is_simply_absent() {
        for line in ["", "not json", "{}", r#"{"report_version":3}"#, "[]"] {
            assert!(
                analysis_info_from_first_line(line).is_none(),
                "should be None for {line:?}"
            );
        }
        let meta = build(&entry(), &Device::Orbic, None, Local::now());
        assert!(meta.analysis.is_none());
        // And the key is dropped entirely rather than serialised as null,
        // which would read as "analysis ran and found no detectors".
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("analysis"), "{json}");
    }

    /// The sidecar goes to other people. It must never be the thing that
    /// leaks the device's identifiers.
    #[test]
    fn no_subscriber_identity_reaches_the_sidecar() {
        let meta = build(&entry(), &Device::Orbic, None, Local::now());
        let json = serde_json::to_string(&meta).unwrap().to_lowercase();
        for forbidden in ["imsi", "imei", "tmsi", "password", "wifi"] {
            assert!(!json.contains(forbidden), "{forbidden} leaked into {json}");
        }
    }
}
