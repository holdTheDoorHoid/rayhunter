//! `telemetry.json`: what the summary bundle says about the recording, and
//! what the service indexes.
//!
//! The daemon fills this in from things it already records: the analysis
//! report, the device details sidecar, the cell tracker's view of the
//! capture, and the location file. The collector reads it out of the
//! decrypted summary part, and the publisher builds the list and the map
//! from it. Every field is best effort and defaults when missing, so a
//! collector from a later version still reads an older unit's bundle.

use serde::{Deserialize, Serialize};

use crate::manifest::{Consent, Tier};

/// How the location in a bundle was reduced before it left the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum LocationPrecision {
    /// No location is sent.
    #[default]
    None,
    /// Rounded to a tenth of a degree, about ten kilometres.
    Coarse,
    /// Rounded to a hundredth of a degree, about one kilometre.
    Neighborhood,
    /// As recorded.
    Exact,
}

impl LocationPrecision {
    /// Round a coordinate to this precision. `None` gives no coordinate.
    pub fn apply(&self, value: f64) -> Option<f64> {
        match self {
            LocationPrecision::None => None,
            LocationPrecision::Coarse => Some((value * 10.0).round() / 10.0),
            LocationPrecision::Neighborhood => Some((value * 100.0).round() / 100.0),
            LocationPrecision::Exact => Some(value),
        }
    }

    /// Roughly how far a published point may be from the true one, in
    /// metres, for the site to say so.
    pub fn radius_metres(&self) -> Option<u32> {
        match self {
            LocationPrecision::None => None,
            LocationPrecision::Coarse => Some(8_000),
            LocationPrecision::Neighborhood => Some(800),
            LocationPrecision::Exact => Some(0),
        }
    }
}

/// One point, already reduced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub precision: LocationPrecision,
    pub latitude: f64,
    pub longitude: f64,
    /// `"gps_api"`, `"fixed"`, or whatever the unit recorded.
    #[serde(default)]
    pub source: String,
    /// How many fixes the recording had, so one point can be told from a
    /// track that was reduced to one.
    #[serde(default)]
    pub fix_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingMeta {
    /// The unit's name for the recording, its start second. Not a secret,
    /// and useful for matching a withdrawal to a file.
    pub id: String,
    pub started: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
    pub qmdl_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceMeta {
    /// Rayhunter's name for the device type, for example `orbic`.
    pub device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_build: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoftwareMeta {
    pub rayhunter_version: String,
    pub system_os: String,
    pub arch: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzerMeta {
    pub name: String,
    pub version: u32,
}

/// One warning, as the analysis report had it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_num: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// `Low`, `Medium` or `High`.
    pub severity: String,
    /// The detector's name, matched by position against the report header.
    pub analyzer: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WarningCounts {
    pub low: u32,
    pub medium: u32,
    pub high: u32,
}

impl WarningCounts {
    pub fn total(&self) -> u32 {
        self.low + self.medium + self.high
    }

    /// The highest severity present, as the report writes it.
    pub fn max_severity(&self) -> Option<&'static str> {
        if self.high > 0 {
            Some("High")
        } else if self.medium > 0 {
            Some("Medium")
        } else if self.low > 0 {
            Some("Low")
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisMeta {
    pub report_version: u32,
    pub analyzers: Vec<AnalyzerMeta>,
    pub warnings: WarningCounts,
    pub events: Vec<EventMeta>,
}

/// A cell the unit heard during the recording.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tac: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<u32>,
    /// Absent when the cell's identity arrived before any measurement had
    /// named a serving cell on its channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pci: Option<u16>,
    pub earfcn: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    /// Only in the full tier: signal strength with a cell identity places a
    /// person, so the summary tier leaves it out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_rsrp_dbm: Option<f32>,
}

/// What the redaction pass removed, copied from the redaction report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionCounts {
    pub imsi: u32,
    pub imei: u32,
    pub imeisv: u32,
    pub tmsi: u32,
    pub messages_scanned: u32,
}

/// The document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub format: String,
    pub submission_id: String,
    pub tier: Option<Tier>,
    pub consent: Option<Consent>,
    pub recording: RecordingMeta,
    pub device: DeviceMeta,
    pub software: SoftwareMeta,
    pub analysis: AnalysisMeta,
    pub cells: Vec<CellMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redaction: Option<RedactionCounts>,
    /// The file names inside the bundle.
    pub contents: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_rounds_as_documented() {
        let lat = 37.774929;
        assert_eq!(LocationPrecision::None.apply(lat), None);
        assert_eq!(LocationPrecision::Coarse.apply(lat), Some(37.8));
        assert_eq!(LocationPrecision::Neighborhood.apply(lat), Some(37.77));
        assert_eq!(LocationPrecision::Exact.apply(lat), Some(lat));
        // Negative coordinates round the same way.
        assert_eq!(LocationPrecision::Coarse.apply(-122.419416), Some(-122.4));
        assert_eq!(
            LocationPrecision::Neighborhood.apply(-122.419416),
            Some(-122.42)
        );
    }

    #[test]
    fn a_sparse_document_from_an_older_unit_still_reads() {
        let s: Summary = serde_json::from_str(r#"{"format":"rayhunter-telemetry/1"}"#).unwrap();
        assert_eq!(s.format, "rayhunter-telemetry/1");
        assert!(s.cells.is_empty());
        assert!(s.location.is_none());
        assert_eq!(s.analysis.warnings.max_severity(), None);
    }

    #[test]
    fn warning_counts_name_the_worst() {
        let mut w = WarningCounts::default();
        assert_eq!(w.max_severity(), None);
        w.low = 2;
        assert_eq!(w.max_severity(), Some("Low"));
        w.high = 1;
        assert_eq!(w.max_severity(), Some("High"));
        assert_eq!(w.total(), 3);
    }
}
