//! Deciding whether a recording may be contributed, and building what is
//! sent when it is.
//!
//! The summary bundle is built from the same functions as the shareable
//! zip: the redacted PCAP from `crate::pcap`, the device details with the
//! identifying parts removed from `RecordingSidecar::redacted`, and the
//! export sidecar from `crate::export_metadata`. Nothing about redaction is
//! reimplemented here; this module decides what goes in and adds the
//! `telemetry.json` the service indexes.
//!
//! **What never leaves.** The recording being written, anything containing
//! a demo warning, anything the owner excluded, anything already sent, and
//! (in either tier) the WiFi network name. See `telemetry/DESIGN.md` for
//! the full table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow};
use async_zip::tokio::write::ZipFileWriter;
use async_zip::{Compression, ZipEntryBuilder};
use chrono::{DateTime, FixedOffset, Local, TimeDelta};
use log::{info, warn};
use rayhunter::Device;
use rayhunter::analysis::analyzer::{AnalysisRow, EventType, ReportMetadata};
use rayhunter::analysis::information_element::InformationElement;
use rayhunter::diag::Message;
use rayhunter::diag::diaglog::LogBody;
use rayhunter::qmdl::QmdlMessageReader;
use rayhunter::recording_metadata::RecordingSidecar;
use telemetry_format::manifest::{Consent, Tier};
use telemetry_format::summary::{
    AnalysisMeta, AnalyzerMeta, CellMeta, DeviceMeta, EventMeta, Location, LocationPrecision,
    RecordingMeta, RedactionCounts, SoftwareMeta, Summary, WarningCounts,
};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, copy};
use tokio::sync::RwLock;
use tokio_util::compat::FuturesAsyncWriteCompatExt;

use crate::cell_info::{band_for_earfcn, identity_from_information_element};
use crate::config::TelemetryConfig;
use crate::demo::DEMO_PREFIX;
use crate::gps::{GpsRecord, load_gps_records};
use crate::pcap::generate_redacted_pcap_data;
use crate::qmdl_store::{FileKind, ManifestEntry, RecordingStore};

/// Why a recording is not contributed. Shown on the settings page, because
/// "it never uploaded" with no reason is the kind of silence that gets a
/// feature switched off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Current,
    Empty,
    Excluded,
    AlreadySent,
    Withdrawn,
    TooYoung,
    NotAnalysed,
    Demo,
    NoWarning,
    BelowMinimumSeverity,
}

impl Skip {
    pub fn reason(&self) -> &'static str {
        match self {
            Skip::Current => "still being recorded",
            Skip::Empty => "empty",
            Skip::Excluded => "excluded by you",
            Skip::AlreadySent => "already contributed",
            Skip::Withdrawn => "withdrawn, so never sent again",
            Skip::TooYoung => "waiting for the minimum age",
            Skip::NotAnalysed => "not analysed yet",
            Skip::Demo => "contains a demo warning",
            Skip::NoWarning => "raised no warning",
            Skip::BelowMinimumSeverity => "below the minimum severity",
        }
    }
}

/// The checks that need nothing but the manifest.
pub fn check_manifest(
    entry: &ManifestEntry,
    is_current: bool,
    min_age: TimeDelta,
    now: DateTime<Local>,
) -> Result<(), Skip> {
    if is_current {
        return Err(Skip::Current);
    }
    if entry.qmdl_size_bytes == 0 {
        return Err(Skip::Empty);
    }
    if entry.telemetry_excluded {
        return Err(Skip::Excluded);
    }
    if let Some(submission) = &entry.telemetry_submission {
        return Err(if submission.withdrawn_at.is_some() {
            Skip::Withdrawn
        } else {
            Skip::AlreadySent
        });
    }
    let age = now - entry.last_message_time.unwrap_or(entry.start_time);
    if age < min_age {
        return Err(Skip::TooYoung);
    }
    Ok(())
}

/// What the analysis report says, as far as contribution cares.
#[derive(Debug, Default)]
pub struct ReportFacts {
    pub header: Option<ReportMetadata>,
    pub events: Vec<EventMeta>,
    pub counts: WarningCounts,
    pub has_demo: bool,
    /// When the first warning happened, for choosing a location point.
    pub first_warning: Option<DateTime<FixedOffset>>,
}

/// Read the report. Rows that cannot be parsed are skipped rather than
/// failing the whole thing, as the interface does.
pub async fn read_report(file: File) -> ReportFacts {
    let mut facts = ReportFacts::default();
    let mut lines = BufReader::new(file).lines();
    let mut first = true;
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        if first {
            first = false;
            facts.header = serde_json::from_str::<ReportMetadata>(&line).ok();
            continue;
        }
        let Ok(row) = serde_json::from_str::<AnalysisRow>(&line) else {
            continue;
        };
        for (index, event) in row.events.iter().enumerate() {
            let Some(event) = event else { continue };
            if event.message.contains(DEMO_PREFIX) {
                facts.has_demo = true;
            }
            match event.event_type {
                EventType::Informational => continue,
                EventType::Low => facts.counts.low += 1,
                EventType::Medium => facts.counts.medium += 1,
                EventType::High => facts.counts.high += 1,
            }
            if facts.first_warning.is_none() {
                facts.first_warning = row.packet_timestamp;
            }
            let analyzer = facts
                .header
                .as_ref()
                .and_then(|h| h.analyzers.get(index))
                .map(|a| a.name.clone())
                .unwrap_or_else(|| format!("analyzer {index}"));
            facts.events.push(EventMeta {
                packet_num: row.packet_num.map(|n| n as u64),
                timestamp: row.packet_timestamp.map(|t| t.to_rfc3339()),
                severity: format!("{:?}", event.event_type),
                analyzer,
                message: event.message.clone(),
            });
        }
    }
    facts
}

/// Whether the report's warnings meet the owner's threshold.
pub fn check_report(facts: &ReportFacts, config: &TelemetryConfig) -> Result<(), Skip> {
    if facts.has_demo {
        return Err(Skip::Demo);
    }
    if facts.counts.total() == 0 {
        return if config.include_clean_recordings {
            Ok(())
        } else {
            Err(Skip::NoWarning)
        };
    }
    let worst = if facts.counts.high > 0 {
        EventType::High
    } else if facts.counts.medium > 0 {
        EventType::Medium
    } else {
        EventType::Low
    };
    if worst < config.min_severity.as_event_type() {
        return Err(Skip::BelowMinimumSeverity);
    }
    Ok(())
}

/// The cells heard during a recording, from the capture itself.
///
/// The physical identity and channel come from the modem's serving-cell
/// measurements; the network's own identity for the cell comes from SIB1,
/// which is attached to whichever cell was serving on that channel when it
/// arrived. Signal strength is kept only when asked for.
pub async fn read_cells<R>(mut reader: QmdlMessageReader<R>, include_signal: bool) -> Vec<CellMeta>
where
    R: tokio::io::AsyncRead + tokio::io::AsyncSeek + Unpin,
{
    #[derive(Default)]
    struct Seen {
        identity: Option<crate::cell_info::CellIdentity>,
        first: Option<DateTime<FixedOffset>>,
        last: Option<DateTime<FixedOffset>>,
        best_rsrp: Option<f32>,
    }
    let mut cells: BTreeMap<(Option<u16>, u32), Seen> = BTreeMap::new();
    let mut serving: Option<(u16, u32)> = None;

    loop {
        let next = match reader.get_next_message().await {
            Ok(Some(Ok(message))) => message,
            Ok(Some(Err(_))) => continue,
            Ok(None) => break,
            Err(e) => {
                warn!("stopped reading cells from the capture: {e}");
                break;
            }
        };
        let Message::Log {
            ref body,
            ref timestamp,
            ..
        } = next
        else {
            continue;
        };
        let when = timestamp.to_datetime();
        match body {
            LogBody::LteMl1ServingCellMeasurementAndEvaluation { data } => {
                let key = (Some(data.get_pci()), data.get_earfcn());
                serving = Some((data.get_pci(), data.get_earfcn()));
                let rsrp = data.get_meas_rsrp();
                let seen = cells.entry(key).or_default();
                seen.first.get_or_insert(when);
                seen.last = Some(when);
                if include_signal {
                    seen.best_rsrp = Some(seen.best_rsrp.map_or(rsrp, |b| b.max(rsrp)));
                }
            }
            LogBody::LteRrcOtaMessage { packet, .. } => {
                let earfcn = packet.get_earfcn();
                let Ok(Some((_, gsmtap))) = rayhunter::gsmtap::parser::parse(next) else {
                    continue;
                };
                let Ok(element) = InformationElement::try_from(&gsmtap) else {
                    continue;
                };
                let Some(identity) = identity_from_information_element(&element) else {
                    continue;
                };
                // SIB1 names the cell that broadcast it, which is the serving
                // cell on that channel. Before any measurement has named a
                // serving cell, the channel alone is what is known.
                let key = match serving {
                    Some((pci, e)) if e == earfcn => (Some(pci), earfcn),
                    _ => (None, earfcn),
                };
                let seen = cells.entry(key).or_default();
                seen.first.get_or_insert(when);
                seen.last = Some(when);
                seen.identity = Some(identity);
            }
            _ => {}
        }
    }

    cells
        .into_iter()
        .map(|((pci, earfcn), seen)| CellMeta {
            mcc: seen.identity.as_ref().and_then(|i| i.mcc.clone()),
            mnc: seen.identity.as_ref().and_then(|i| i.mnc.clone()),
            tac: seen.identity.as_ref().and_then(|i| i.tac),
            cell_id: seen.identity.as_ref().and_then(|i| i.cell_id),
            pci,
            earfcn,
            band: band_for_earfcn(earfcn),
            first_seen: seen.first.map(|t| t.to_rfc3339()),
            last_seen: seen.last.map(|t| t.to_rfc3339()),
            best_rsrp_dbm: seen.best_rsrp,
        })
        .collect()
}

/// Round every fix to the chosen precision and drop runs of identical
/// points, which is what a track looks like once it has been coarsened.
pub fn reduce_track(records: &[GpsRecord], precision: LocationPrecision) -> Vec<GpsRecord> {
    let mut out: Vec<GpsRecord> = Vec::new();
    for r in records {
        let (Some(lat), Some(lon)) = (precision.apply(r.lat), precision.apply(r.lon)) else {
            continue;
        };
        if let Some(prev) = out.last()
            && prev.lat == lat
            && prev.lon == lon
        {
            continue;
        }
        out.push(GpsRecord {
            latest_packet_timestamp: r.latest_packet_timestamp,
            system_time: r.system_time,
            lat,
            lon,
        });
    }
    out
}

/// One point for the map: the fix nearest the first warning, or the middle
/// of the track when there was no warning.
pub fn choose_point(records: &[GpsRecord], anchor_unix: Option<i64>) -> Option<&GpsRecord> {
    if records.is_empty() {
        return None;
    }
    if let Some(anchor) = anchor_unix {
        let nearest = records
            .iter()
            .filter(|r| r.latest_packet_timestamp.is_some())
            .min_by_key(|r| {
                (i128::from(r.latest_packet_timestamp.unwrap_or(0)) - i128::from(anchor)).abs()
            });
        if nearest.is_some() {
            return nearest;
        }
    }
    records.get(records.len() / 2)
}

pub fn summary_location(
    records: &[GpsRecord],
    precision: LocationPrecision,
    anchor: Option<DateTime<FixedOffset>>,
    source: &str,
) -> Option<Location> {
    let point = choose_point(records, anchor.map(|t| t.timestamp()))?;
    Some(Location {
        precision,
        latitude: precision.apply(point.lat)?,
        longitude: precision.apply(point.lon)?,
        source: source.to_string(),
        fix_count: records.len() as u32,
    })
}

/// Writes entries into a zip and remembers their names.
struct Bundle<W: AsyncWrite + Unpin> {
    zip: ZipFileWriter<W>,
    contents: Vec<String>,
}

impl<W: AsyncWrite + Unpin> Bundle<W> {
    fn new(writer: W) -> Self {
        Bundle {
            zip: ZipFileWriter::with_tokio(writer),
            contents: Vec::new(),
        }
    }

    fn entry(name: &str) -> ZipEntryBuilder {
        // An explicit mode: without one, some unzip tools extract every
        // file unreadable.
        ZipEntryBuilder::new(name.to_string().into(), Compression::Stored).unix_permissions(0o644)
    }

    async fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let mut writer = self
            .zip
            .write_entry_stream(Self::entry(name))
            .await?
            .compat_write();
        writer.write_all(bytes).await?;
        writer.into_inner().close().await?;
        self.contents.push(name.to_string());
        Ok(())
    }

    async fn add_file(&mut self, name: &str, file: &mut File) -> anyhow::Result<()> {
        let mut writer = self
            .zip
            .write_entry_stream(Self::entry(name))
            .await?
            .compat_write();
        copy(file, &mut writer).await?;
        writer.into_inner().close().await?;
        self.contents.push(name.to_string());
        Ok(())
    }

    async fn finish(self) -> anyhow::Result<Vec<String>> {
        self.zip.close().await?;
        Ok(self.contents)
    }
}

/// What to build.
pub struct Plan<'a> {
    pub config: &'a TelemetryConfig,
    pub device: &'a Device,
    pub submission_id: String,
    pub consent: Consent,
}

/// What was built, on disk under the spool directory.
pub struct Built {
    pub summary_zip: PathBuf,
    pub capture_zip: Option<PathBuf>,
    pub summary: Summary,
}

async fn open(
    store: &Arc<RwLock<RecordingStore>>,
    name: &str,
    kind: FileKind,
) -> anyhow::Result<Option<File>> {
    let store = store.read().await;
    let (index, _) = store
        .entry_for_name(name)
        .ok_or_else(|| anyhow!("recording {name} is gone"))?;
    Ok(store.open_file(index, kind).await?)
}

/// Build the bundle(s) for one recording into `spool`, which must exist and
/// be empty. `facts` is the report as already read for the eligibility
/// check, so it is not read twice.
pub async fn build(
    store: &Arc<RwLock<RecordingStore>>,
    entry: &ManifestEntry,
    facts: &ReportFacts,
    plan: &Plan<'_>,
    spool: &Path,
) -> anyhow::Result<Built> {
    let name = entry.name.as_str();
    let tier = plan.config.tier;
    let precision = plan.config.location;
    let full = tier == Tier::Full;

    // The device details, with what identifies the owner removed. The WiFi
    // block goes in both tiers: the network's name is where the owner lives.
    let sidecar: Option<RecordingSidecar> = match open(store, name, FileKind::Meta).await? {
        Some(mut file) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).await?;
            serde_json::from_slice(&bytes).ok()
        }
        None => None,
    };
    let shared_sidecar = sidecar.as_ref().map(|s| {
        if full {
            let mut copy = s.clone();
            if copy.wifi.is_some() {
                copy.wifi = None;
                copy.redacted_fields.push("wifi".to_string());
            }
            copy
        } else {
            s.redacted()
        }
    });

    // Location, reduced before anything else sees it. The PCAP writer embeds
    // fixes in the capture, so it must be handed the reduced track and never
    // the recorded one.
    let raw_track = match open(store, name, FileKind::Gps).await? {
        Some(file) => load_gps_records(file).await,
        None => Vec::new(),
    };
    let track = reduce_track(&raw_track, precision);
    let source = match entry.gps_mode {
        Some(crate::config::GpsMode::Api) => "gps_api",
        Some(crate::config::GpsMode::Fixed) => "fixed",
        _ => "none",
    };
    let location = summary_location(&raw_track, precision, facts.first_warning, source);

    // Cells, from a pass over the capture.
    let cells = match open(store, name, FileKind::Qmdl).await? {
        Some(file) => read_cells(QmdlMessageReader::new(file).await?, full).await,
        None => Vec::new(),
    };

    // The export sidecar, without the owner's words unless asked for.
    let mut export_entry = entry.clone();
    if !(full && plan.config.include_notes) {
        export_entry.display_name = None;
        export_entry.notes = None;
    }
    let analysis_info = facts
        .header
        .as_ref()
        .map(|h| crate::export_metadata::AnalysisInfo {
            report_version: h.report_version,
            analyzers: h
                .analyzers
                .iter()
                .map(|a| crate::export_metadata::AnalyzerInfo {
                    name: a.name.clone(),
                    version: a.version,
                })
                .collect(),
        });
    let export_metadata = crate::export_metadata::build(
        &export_entry,
        plan.device,
        analysis_info,
        rayhunter::clock::get_adjusted_now(),
    );
    let export_json = serde_json::to_vec_pretty(&export_metadata)?;

    let mut summary = Summary {
        format: telemetry_format::FORMAT.to_string(),
        submission_id: plan.submission_id.clone(),
        tier: Some(tier),
        consent: Some(plan.consent.clone()),
        recording: RecordingMeta {
            id: entry.name.clone(),
            started: entry.start_time.to_rfc3339(),
            ended: entry.last_message_time.map(|t| t.to_rfc3339()),
            qmdl_size_bytes: entry.qmdl_size_bytes as u64,
            stop_reason: entry.stop_reason.clone(),
        },
        device: DeviceMeta {
            device: sidecar
                .as_ref()
                .map(|s| s.hardware.device.clone())
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| format!("{:?}", plan.device).to_lowercase()),
            model: sidecar.as_ref().and_then(|s| s.hardware.model.clone()),
            hardware_version: sidecar
                .as_ref()
                .and_then(|s| s.hardware.hardware_version.clone()),
            soc: sidecar.as_ref().and_then(|s| s.hardware.soc.clone()),
            firmware_build: sidecar
                .as_ref()
                .and_then(|s| s.hardware.firmware_build.clone()),
        },
        software: SoftwareMeta {
            rayhunter_version: entry
                .rayhunter_version
                .clone()
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
            system_os: entry.system_os.clone().unwrap_or_default(),
            arch: entry.arch.clone().unwrap_or_default(),
        },
        analysis: AnalysisMeta {
            report_version: facts.header.as_ref().map(|h| h.report_version).unwrap_or(0),
            analyzers: facts
                .header
                .as_ref()
                .map(|h| {
                    h.analyzers
                        .iter()
                        .map(|a| AnalyzerMeta {
                            name: a.name.clone(),
                            version: a.version,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            warnings: facts.counts.clone(),
            events: facts.events.clone(),
        },
        cells,
        location,
        redaction: None,
        contents: Vec::new(),
    };

    // The summary bundle.
    let summary_zip = spool.join("summary.zip");
    {
        let file = File::create(&summary_zip).await?;
        let mut bundle = Bundle::new(file);

        // The redacted PCAP, with the reduced track. A summary whose PCAP
        // failed to build has nothing in it worth sending.
        {
            let qmdl = open(store, name, FileKind::Qmdl)
                .await?
                .ok_or_else(|| anyhow!("the capture file is missing"))?;
            let reader = QmdlMessageReader::new(qmdl).await?;
            let pcap_name = format!("{name}.pcapng");
            let mut writer = bundle
                .zip
                .write_entry_stream(Bundle::<File>::entry(&pcap_name))
                .await?
                .compat_write();
            let report = generate_redacted_pcap_data(&mut writer, reader, track.clone())
                .await
                .context("building the redacted capture")?;
            writer.into_inner().close().await?;
            bundle.contents.push(pcap_name);
            info!(
                "contribution {}: removed {} identifiers from {} messages",
                entry.name,
                report.total(),
                report.messages_scanned
            );
            summary.redaction = Some(RedactionCounts {
                imsi: report.imsi as u32,
                imei: report.imei as u32,
                imeisv: report.imeisv as u32,
                tmsi: report.tmsi as u32,
                messages_scanned: report.messages_scanned as u32,
            });
            bundle
                .add_bytes(
                    "redaction-report.json",
                    &serde_json::to_vec_pretty(&report)?,
                )
                .await?;
        }

        if let Some(mut analysis) = open(store, name, FileKind::Analysis).await? {
            bundle
                .add_file(&format!("{name}.ndjson"), &mut analysis)
                .await?;
        }
        if let Some(shared) = &shared_sidecar {
            bundle
                .add_bytes(
                    &FileKind::Meta.get_filename(name, false),
                    &serde_json::to_vec_pretty(shared)?,
                )
                .await?;
        }
        bundle.add_bytes("metadata.json", &export_json).await?;
        if let Some(location) = &summary.location {
            bundle
                .add_bytes("location.json", &serde_json::to_vec_pretty(location)?)
                .await?;
        }
        // The document goes in last so it can list everything before it.
        let mut listing = bundle.contents.clone();
        listing.push("telemetry.json".to_string());
        summary.contents = listing;
        bundle
            .add_bytes("telemetry.json", &serde_json::to_vec_pretty(&summary)?)
            .await?;
        bundle.finish().await?;
    }

    // The capture bundle, full tier only.
    let capture_zip = if full {
        let path = spool.join("capture.zip");
        let file = File::create(&path).await?;
        let mut bundle = Bundle::new(file);
        {
            let mut qmdl = open(store, name, FileKind::Qmdl)
                .await?
                .ok_or_else(|| anyhow!("the capture file is missing"))?;
            let stored_name = FileKind::Qmdl.get_filename(name, entry.compressed);
            bundle.add_file(&stored_name, &mut qmdl).await?;
        }
        if !track.is_empty() {
            let mut lines = String::new();
            for record in &track {
                lines.push_str(&serde_json::to_string(record)?);
                lines.push('\n');
            }
            bundle
                .add_bytes(&FileKind::Gps.get_filename(name, false), lines.as_bytes())
                .await?;
        }
        if let Some(shared) = &shared_sidecar {
            bundle
                .add_bytes(
                    &FileKind::Meta.get_filename(name, false),
                    &serde_json::to_vec_pretty(shared)?,
                )
                .await?;
        }
        bundle.add_bytes("metadata.json", &export_json).await?;
        bundle
            .add_bytes("telemetry.json", &serde_json::to_vec_pretty(&summary)?)
            .await?;
        bundle.finish().await?;
        Some(path)
    } else {
        None
    };

    Ok(Built {
        summary_zip,
        capture_zip,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TelemetryConfig, TelemetrySeverity};

    fn rec(ts: i64, lat: f64, lon: f64) -> GpsRecord {
        GpsRecord {
            latest_packet_timestamp: Some(ts),
            system_time: ts,
            lat,
            lon,
        }
    }

    fn entry(name: &str) -> ManifestEntry {
        ManifestEntry {
            name: name.into(),
            start_time: Local::now() - TimeDelta::hours(2),
            last_message_time: Some(Local::now() - TimeDelta::hours(2)),
            qmdl_size_bytes: 10,
            rayhunter_version: None,
            system_os: None,
            arch: None,
            stop_reason: None,
            upload_time: None,
            gps_mode: None,
            compressed: true,
            display_name: None,
            notes: None,
            telemetry_submission: None,
            telemetry_excluded: false,
        }
    }

    #[test]
    fn manifest_checks_name_their_reason() {
        let now = Local::now();
        let hour = TimeDelta::hours(1);
        assert_eq!(check_manifest(&entry("a"), false, hour, now), Ok(()));
        assert_eq!(
            check_manifest(&entry("a"), true, hour, now),
            Err(Skip::Current)
        );
        let mut e = entry("a");
        e.qmdl_size_bytes = 0;
        assert_eq!(check_manifest(&e, false, hour, now), Err(Skip::Empty));
        let mut e = entry("a");
        e.telemetry_excluded = true;
        assert_eq!(check_manifest(&e, false, hour, now), Err(Skip::Excluded));
        let mut e = entry("a");
        e.telemetry_submission = Some(crate::qmdl_store::TelemetrySubmission {
            submission_id: "x".into(),
            tier: Tier::Summary,
            submitted_at: now,
            key_id: "k".into(),
            server_url: "https://example".into(),
            withdrawn_at: None,
        });
        assert_eq!(check_manifest(&e, false, hour, now), Err(Skip::AlreadySent));
        e.telemetry_submission.as_mut().unwrap().withdrawn_at = Some(now);
        assert_eq!(check_manifest(&e, false, hour, now), Err(Skip::Withdrawn));
        let mut e = entry("a");
        e.last_message_time = Some(now - TimeDelta::minutes(5));
        assert_eq!(check_manifest(&e, false, hour, now), Err(Skip::TooYoung));
    }

    #[tokio::test]
    async fn the_report_is_read_and_demo_warnings_are_noticed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.ndjson");
        let header = r#"{"analyzers":[{"name":"IMSI Requested","description":"d","version":4},{"name":"Null Cipher","description":"d","version":2}],"rayhunter":{"rayhunter_version":"0.12.3","system_os":"linux","arch":"arm"},"report_version":3}"#;
        let row1 = r#"{"packet_num":7,"packet_timestamp":"2026-09-02T10:00:00+00:00","skipped_message_reason":null,"events":[{"event_type":"Medium","message":"Identity requested"},null]}"#;
        let row2 = r#"{"packet_num":9,"packet_timestamp":"2026-09-02T10:00:05+00:00","skipped_message_reason":null,"events":[null,{"event_type":"High","message":"Null cipher"}]}"#;
        let row3 = r#"{"packet_num":11,"packet_timestamp":"2026-09-02T10:00:06+00:00","skipped_message_reason":null,"events":[{"event_type":"Informational","message":"routine"},null]}"#;
        tokio::fs::write(
            &path,
            format!("{header}\n{row1}\n{row2}\nnot json\n{row3}\n"),
        )
        .await
        .unwrap();
        let facts = read_report(File::open(&path).await.unwrap()).await;
        assert_eq!(facts.counts.medium, 1);
        assert_eq!(facts.counts.high, 1);
        assert_eq!(
            facts.events.len(),
            2,
            "informational events are not warnings"
        );
        assert_eq!(facts.events[0].analyzer, "IMSI Requested");
        assert_eq!(facts.events[1].analyzer, "Null Cipher");
        assert_eq!(facts.events[0].packet_num, Some(7));
        assert!(!facts.has_demo);
        assert_eq!(facts.first_warning.map(|t| t.timestamp()), Some(1788343200));

        let demo = format!(
            r#"{{"packet_num":1,"packet_timestamp":"2026-09-02T10:00:00+00:00","skipped_message_reason":null,"events":[{{"event_type":"High","message":"{DEMO_PREFIX}fake"}}]}}"#
        );
        tokio::fs::write(&path, format!("{header}\n{demo}\n"))
            .await
            .unwrap();
        let facts = read_report(File::open(&path).await.unwrap()).await;
        assert!(facts.has_demo);
        let config = TelemetryConfig::default();
        assert_eq!(check_report(&facts, &config), Err(Skip::Demo));
    }

    #[test]
    fn the_severity_gate_and_the_clean_option() {
        let mut config = TelemetryConfig::default();
        let mut facts = ReportFacts::default();
        assert_eq!(check_report(&facts, &config), Err(Skip::NoWarning));
        config.include_clean_recordings = true;
        assert_eq!(check_report(&facts, &config), Ok(()));

        facts.counts.low = 1;
        config.min_severity = TelemetrySeverity::Medium;
        assert_eq!(
            check_report(&facts, &config),
            Err(Skip::BelowMinimumSeverity)
        );
        facts.counts.high = 1;
        assert_eq!(check_report(&facts, &config), Ok(()));
    }

    /// The whole point of the precision setting: what leaves is rounded, and
    /// a track collapses to the points that differ at that precision.
    #[test]
    fn tracks_are_reduced_and_points_chosen_near_the_warning() {
        let track = vec![
            rec(100, 37.774929, -122.419416),
            rec(200, 37.774001, -122.419500),
            rec(300, 37.801234, -122.401234),
        ];
        let coarse = reduce_track(&track, LocationPrecision::Coarse);
        assert_eq!(coarse.len(), 1, "all three round to the same 10 km cell");
        assert_eq!((coarse[0].lat, coarse[0].lon), (37.8, -122.4));
        let fine = reduce_track(&track, LocationPrecision::Neighborhood);
        assert_eq!(fine.len(), 2);
        assert!(reduce_track(&track, LocationPrecision::None).is_empty());
        assert_eq!(reduce_track(&track, LocationPrecision::Exact).len(), 3);

        let near = choose_point(&track, Some(290)).unwrap();
        assert_eq!(near.latest_packet_timestamp, Some(300));
        let middle = choose_point(&track, None).unwrap();
        assert_eq!(middle.latest_packet_timestamp, Some(200));
        assert!(choose_point(&[], Some(1)).is_none());

        let anchor = DateTime::parse_from_rfc3339("1970-01-01T00:04:50+00:00").unwrap();
        let loc =
            summary_location(&track, LocationPrecision::Coarse, Some(anchor), "gps_api").unwrap();
        assert_eq!((loc.latitude, loc.longitude), (37.8, -122.4));
        assert_eq!(loc.fix_count, 3);
        assert!(summary_location(&track, LocationPrecision::None, None, "gps_api").is_none());
    }
}
