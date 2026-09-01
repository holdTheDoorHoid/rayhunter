//! Making room by removing recordings that found nothing.
//!
//! A device left running fills its storage and then stops recording, which is
//! the moment it stops being a detector. Most recordings find nothing, and a
//! recording that found nothing is the one thing here safe to lose: the
//! interesting ones are exactly the ones this never touches.
//!
//! Off by default. Deleting somebody's captures without being asked is not a
//! thing to do quietly, however good the reason.

use log::{info, warn};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::qmdl_store::{FileKind, RecordingStore};
use crate::stats::DiskStats;

/// What a recording is worth keeping for, as far as pruning is concerned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    /// Analysed, nothing found. Safe to remove to make room.
    Clean,
    /// Something was found. Never removed automatically.
    HasWarnings,
    /// Left alone. Not analysed, still recording, unreadable, waiting to be
    /// uploaded, or named by someone. Not knowing what is in a recording is
    /// not a reason to delete it, and neither is somebody having labelled it.
    Unknown,
}

/// How a single line of an analysis report reads, for pruning purposes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineClass {
    /// A well-formed line carrying at least one non-informational event.
    Warning,
    /// A well-formed line with no warning: metadata, a clean row, or blank.
    NoWarning,
    /// A line that could not be parsed. We cannot say what it means.
    Malformed,
}

/// Classify one line of an analysis report.
///
/// The safety property is that a line nobody can parse must be distinguishable
/// from a clean one. A truncated write, a corrupt row, or an
/// as-yet-unrecognised format must not let a recording read as "found nothing"
/// and then be deleted to make room. So every non-blank line is parsed as JSON,
/// and anything that does not parse is [`LineClass::Malformed`] rather than
/// silently treated as no-warning.
///
/// Informational events do not count as warnings. They are diagnostics rather
/// than detections, and a recording carrying only those still found nothing.
pub fn classify_line(line: &str) -> LineClass {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return LineClass::NoWarning;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return LineClass::Malformed;
    };
    let has_warning = value
        .get("events")
        .and_then(|e| e.as_array())
        .is_some_and(|events| {
            events.iter().any(|event| {
                event
                    .get("event_type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| t != "Informational")
            })
        });
    if has_warning {
        LineClass::Warning
    } else {
        LineClass::NoWarning
    }
}

/// Decide whether one recording can be removed to make room.
async fn verdict_for(store: &RecordingStore, index: usize, uploads_configured: bool) -> Verdict {
    let Some(entry) = store.manifest.entries.get(index) else {
        return Verdict::Unknown;
    };

    // Never touch the recording being written.
    if store.is_current_entry(&entry.name) {
        return Verdict::Unknown;
    }

    // A recording waiting to be uploaded has not reached anywhere else yet, so
    // removing it here would lose it outright. Only relevant when uploads are
    // actually set up; otherwise every recording would look pending forever.
    if uploads_configured && entry.upload_time.is_none() {
        return Verdict::Unknown;
    }

    // Somebody who stopped to name a recording or write notes about it has
    // said it matters to them, whatever the analysers made of it. That is a
    // clearer signal of worth than anything measurable here, so it wins.
    if entry.display_name.is_some() || entry.notes.is_some() {
        return Verdict::Unknown;
    }

    let Ok(Some(file)) = store.open_file(index, FileKind::Analysis).await else {
        // No report means it has not been analysed, or the analysis failed.
        // Either way nobody knows what is in it, so it stays.
        return Verdict::Unknown;
    };

    let mut lines = BufReader::new(file).lines();
    let mut saw_a_line = false;
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if !line.trim().is_empty() {
                    saw_a_line = true;
                }
                match classify_line(&line) {
                    LineClass::Warning => return Verdict::HasWarnings,
                    // A line nobody can parse means we cannot certify the
                    // recording as clean, so it is kept rather than deleted.
                    LineClass::Malformed => {
                        warn!(
                            "analysis for {} has an unparseable line; keeping the recording",
                            entry.name
                        );
                        return Verdict::Unknown;
                    }
                    LineClass::NoWarning => {}
                }
            }
            // Reached the end with every line parsed and none a warning. An
            // empty report proves nothing was analysed, so it is not "clean".
            Ok(None) => {
                return if saw_a_line {
                    Verdict::Clean
                } else {
                    Verdict::Unknown
                };
            }
            Err(e) => {
                warn!("couldn't read the analysis for {}: {e}", entry.name);
                return Verdict::Unknown;
            }
        }
    }
}

/// Free space by deleting clean recordings, oldest first, until there is room.
///
/// Returns how many were removed. Stops as soon as the target is met, so a
/// device that needs one recording's worth of room loses exactly one.
pub async fn prune_clean_recordings(
    store: &mut RecordingStore,
    target_mb: u64,
    uploads_configured: bool,
) -> usize {
    // The path is copied out so that reading free space does not keep a borrow
    // on the store, which has to be mutable to delete anything.
    let path = store.path.to_string_lossy().into_owned();
    let available_mb = move |path: &str| match DiskStats::new(path) {
        Ok(stats) => stats.available_bytes.unwrap_or(0) / 1024 / 1024,
        Err(e) => {
            warn!("couldn't read disk space while pruning: {e}");
            // Reported as plenty, which stops the loop rather than deleting on
            // the strength of a reading that failed.
            u64::MAX
        }
    };

    if available_mb(&path) >= target_mb {
        return 0;
    }

    // Oldest first. The manifest is kept in chronological order, and the
    // oldest clean recording is the least likely to still be wanted.
    let names: Vec<String> = store
        .manifest
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();

    let mut removed = 0;
    for name in names {
        if available_mb(&path) >= target_mb {
            break;
        }
        let Some((index, _)) = store.entry_for_name(&name) else {
            continue;
        };
        if verdict_for(store, index, uploads_configured).await != Verdict::Clean {
            continue;
        }
        match store.delete_entry(&name).await {
            Ok(()) => {
                info!("removed {name} to make room: analysed, no warnings");
                removed += 1;
            }
            Err(e) => warn!("couldn't remove {name} while making room: {e}"),
        }
    }

    if removed == 0 {
        warn!(
            "disk space is low and nothing could be freed automatically: every recording either \
             raised a warning, has been named, has not been analysed, or is still waiting to be \
             uploaded"
        );
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole safety property: a recording that found something is never
    /// the one deleted to make room.
    #[test]
    fn a_warning_on_a_line_is_recognised() {
        let line = r#"{"packet_timestamp":"2024-08-19T03:33:54Z","events":[null,{"event_type":"High","message":"bad"}]}"#;
        assert_eq!(classify_line(line), LineClass::Warning);
        let low = r#"{"events":[{"event_type":"Low","message":"x"}]}"#;
        assert_eq!(classify_line(low), LineClass::Warning);
        let medium = r#"{"events":[{"event_type":"Medium","message":"x"}]}"#;
        assert_eq!(classify_line(medium), LineClass::Warning);
    }

    /// Informational events are diagnostics, not detections. A recording
    /// carrying only those still found nothing, and deleting it is the point
    /// of the feature rather than a failure of it.
    #[test]
    fn informational_events_are_not_warnings() {
        let line = r#"{"events":[{"event_type":"Informational","message":"note"}]}"#;
        assert_eq!(classify_line(line), LineClass::NoWarning);
    }

    #[test]
    fn ordinary_report_lines_are_not_warnings() {
        assert_eq!(
            classify_line(r#"{"skipped_message_reason":"unparsed"}"#),
            LineClass::NoWarning
        );
        assert_eq!(
            classify_line(r#"{"events":[null,null]}"#),
            LineClass::NoWarning
        );
        assert_eq!(classify_line(r#"{"events":[]}"#), LineClass::NoWarning);
        assert_eq!(classify_line(""), LineClass::NoWarning);
    }

    /// The metadata line names every analyzer and their descriptions, which
    /// mention severities in prose. Treating that as a warning would make
    /// every recording look interesting and free nothing at all.
    #[test]
    fn the_metadata_line_is_not_mistaken_for_a_warning() {
        let metadata = r#"{"analyzers":[{"name":"Null Cipher","description":"Tests whether the cell suggests using a null cipher (EEA0)","version":1}],"report_version":3}"#;
        assert_eq!(classify_line(metadata), LineClass::NoWarning);
    }

    /// Malformed input must be Malformed, not silently no-warning. The caller
    /// turns a single malformed line into Verdict::Unknown, which is what keeps
    /// a corrupt or truncated report from being read as "clean" and deleted.
    #[test]
    fn unparseable_lines_are_malformed() {
        assert_eq!(classify_line("{not json"), LineClass::Malformed);
        assert_eq!(classify_line("event_type"), LineClass::Malformed);
        // A row truncated mid-write, which used to read as no-warning.
        assert_eq!(
            classify_line(r#"{"events":[{"event_type":"Hi"#),
            LineClass::Malformed
        );
    }

    async fn store_with_analysis(dir: &std::path::Path, contents: &[u8]) -> RecordingStore {
        use tokio::io::AsyncWriteExt;
        let mut store = RecordingStore::create(dir).await.unwrap();
        let (_qmdl, mut analysis) = store
            .new_entry(crate::config::GpsMode::Disabled)
            .await
            .unwrap();
        analysis.write_all(contents).await.unwrap();
        analysis.flush().await.unwrap();
        store.close_current_entry().await.unwrap();
        store
    }

    /// A report with a malformed line must keep the recording, not delete it.
    #[tokio::test]
    async fn a_malformed_report_is_unknown_not_clean() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_analysis(
            dir.path(),
            b"{\"report_version\":3}\n{\"events\":[null]}\n{ truncated and broken\n",
        )
        .await;
        assert_eq!(verdict_for(&store, 0, false).await, Verdict::Unknown);
    }

    /// An empty analysis report proves nothing was analysed, so it is not clean.
    #[tokio::test]
    async fn an_empty_report_is_unknown_not_clean() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_analysis(dir.path(), b"").await;
        assert_eq!(verdict_for(&store, 0, false).await, Verdict::Unknown);
    }

    /// A well-formed report with no warnings is the one case that may be pruned.
    #[tokio::test]
    async fn a_clean_finished_report_is_clean() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_analysis(
            dir.path(),
            b"{\"report_version\":3}\n{\"skipped_message_reason\":\"unparsed\"}\n",
        )
        .await;
        assert_eq!(verdict_for(&store, 0, false).await, Verdict::Clean);
    }

    /// A warning anywhere in a valid report keeps the recording.
    #[tokio::test]
    async fn a_report_with_a_warning_has_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_analysis(
            dir.path(),
            b"{\"report_version\":3}\n{\"events\":[{\"event_type\":\"High\",\"message\":\"x\"}]}\n",
        )
        .await;
        assert_eq!(verdict_for(&store, 0, false).await, Verdict::HasWarnings);
    }
}
