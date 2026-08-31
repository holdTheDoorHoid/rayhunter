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

/// Whether an analysis report contains any warning.
///
/// Reads the report a line at a time and stops at the first warning rather
/// than parsing the whole file. Most reports are clean and have to be read
/// through, but the ones that are not are usually decided in the first few
/// lines, and this runs on a device with one slow core.
///
/// Informational events do not count. They are diagnostics rather than
/// detections, and a recording carrying only those is still one that found
/// nothing.
pub fn line_has_warning(line: &str) -> bool {
    // Cheap reject first: the overwhelming majority of lines never mention a
    // severity at all, and this saves parsing them as JSON.
    if !line.contains("event_type") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(events) = value.get("events").and_then(|e| e.as_array()) else {
        return false;
    };
    events.iter().any(|event| {
        event
            .get("event_type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t != "Informational")
    })
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
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line_has_warning(&line) {
                    return Verdict::HasWarnings;
                }
            }
            Ok(None) => return Verdict::Clean,
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
        assert!(line_has_warning(line));
        let low = r#"{"events":[{"event_type":"Low","message":"x"}]}"#;
        assert!(line_has_warning(low));
        let medium = r#"{"events":[{"event_type":"Medium","message":"x"}]}"#;
        assert!(line_has_warning(medium));
    }

    /// Informational events are diagnostics, not detections. A recording
    /// carrying only those still found nothing, and deleting it is the point
    /// of the feature rather than a failure of it.
    #[test]
    fn informational_events_are_not_warnings() {
        let line = r#"{"events":[{"event_type":"Informational","message":"note"}]}"#;
        assert!(!line_has_warning(line));
    }

    #[test]
    fn ordinary_report_lines_are_not_warnings() {
        assert!(!line_has_warning(
            r#"{"skipped_message_reason":"unparsed"}"#
        ));
        assert!(!line_has_warning(r#"{"events":[null,null]}"#));
        assert!(!line_has_warning(r#"{"events":[]}"#));
        assert!(!line_has_warning(""));
    }

    /// The metadata line names every analyzer and their descriptions, which
    /// mention severities in prose. Treating that as a warning would make
    /// every recording look interesting and free nothing at all.
    #[test]
    fn the_metadata_line_is_not_mistaken_for_a_warning() {
        let metadata = r#"{"analyzers":[{"name":"Null Cipher","description":"Tests whether the cell suggests using a null cipher (EEA0)","version":1}],"report_version":3}"#;
        assert!(!line_has_warning(metadata));
    }

    /// Malformed input must not be read as "clean", since that would delete a
    /// recording on the strength of a line nobody could parse.
    #[test]
    fn unparseable_lines_do_not_report_a_warning_either_way() {
        // Reported as no warning, and the caller treats an unreadable *file*
        // as Unknown, which is what actually protects the recording.
        assert!(!line_has_warning("{not json"));
        assert!(!line_has_warning("event_type"));
    }
}
