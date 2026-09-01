use std::ops::DerefMut;
use std::pin::pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use futures::{StreamExt, TryStreamExt, future};
use log::{debug, error, info, warn};
use rayhunter::Device;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cell_info::{
    CellTracker, NeighborCell, SignalMeasurements, identity_from_information_element,
    nas_security_from_information_element, rrc_security_from_information_element,
};
use crate::gps::GpsRecord;
use rayhunter::analysis::information_element::InformationElement;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{RwLock, oneshot};
use tokio_stream::wrappers::LinesStream;
use tokio_util::task::TaskTracker;

use rayhunter::DeviceMetadata;
#[cfg(feature = "apidocs")]
use rayhunter::analysis::analyzer::ReportMetadata;
use rayhunter::analysis::analyzer::{AnalysisLineNormalizer, AnalyzerConfig, EventType};
use rayhunter::diag::{DataType, Message, MessagesContainer};
use rayhunter::diag_device::DiagDevice;
use rayhunter::qmdl::QmdlWriter;

use crate::analysis::{AnalysisCtrlMessage, AnalysisWriter};
use crate::config::GpsMode;
use crate::display;
use crate::notifications::{Notification, NotificationType};
use crate::qmdl_store::{FileKind, RecordingStore, RecordingStoreError};
use crate::server::ServerState;
use crate::stats::DiskStats;

const DISK_CHECK_BYTES_INTERVAL: usize = 256 * 1024;

pub enum DiagDeviceCtrlMessage {
    /// Inject a synthetic, clearly labelled warning for demonstration.
    /// Only reachable when demo_mode is enabled in the config.
    InjectDemo,
    StopRecording,
    StartRecording {
        response_tx: Option<oneshot::Sender<Result<(), RecordingStoreError>>>,
    },
    DeleteEntry {
        name: String,
        response_tx: oneshot::Sender<Result<(), RecordingStoreError>>,
    },
    DeleteAllEntries {
        response_tx: oneshot::Sender<Result<(), RecordingStoreError>>,
    },
    GpsUpdate {
        lat: f64,
        lon: f64,
    },
    Exit,
}

pub struct DiagTask {
    ui_update_sender: Sender<display::DisplayState>,
    analysis_sender: Sender<AnalysisCtrlMessage>,
    analyzer_config: AnalyzerConfig,
    device: Device,
    notification_channel: tokio::sync::mpsc::Sender<Notification>,
    min_space_to_start_mb: u64,
    min_space_to_continue_mb: u64,
    /// Whether to remove recordings that found nothing when space runs low.
    auto_delete_clean: bool,
    /// Whether uploads are configured, so a recording that has not been
    /// uploaded yet is never removed to make room.
    uploads_configured: bool,
    /// Rotate to a new recording at this size. `None` never rotates on size.
    max_recording_bytes: Option<u64>,
    /// Rotate to a new recording after this long. `None` never rotates on time.
    max_recording_duration: Option<Duration>,
    /// When the running recording began, for the time based limit. Measured
    /// with a monotonic clock so that a clock correction, which these devices
    /// do apply once they have a network time, cannot make a recording look
    /// hours old the moment it lands.
    recording_started_at: Option<Instant>,
    gps_mode: GpsMode,
    gps_fixed_coords: Option<(f64, f64)>,
    state: DiagState,
    max_type_seen: EventType,
    bytes_since_space_check: usize,
    low_space_warned: bool,
    latest_packet_timestamp: Option<i64>,
    /// Shared with the server so the web UI can read what the radio sees.
    cell_tracker: Arc<RwLock<CellTracker>>,
}

enum DiagState {
    Recording {
        qmdl_writer: Box<QmdlWriter<File>>,
        analysis_writer: Box<AnalysisWriter>,
    },
    Stopped,
}

enum DiskSpaceCheck {
    Ok(u64),
    Warning(u64),
    Critical(u64),
    Failed,
}

fn check_disk_space(path: &std::path::Path, warning_mb: u64, critical_mb: u64) -> DiskSpaceCheck {
    match DiskStats::new(path.to_str().unwrap()) {
        Ok(stats) => {
            let available_mb = stats.available_bytes.unwrap_or(0) / 1024 / 1024;
            if available_mb < critical_mb {
                DiskSpaceCheck::Critical(available_mb)
            } else if available_mb < warning_mb {
                DiskSpaceCheck::Warning(available_mb)
            } else {
                DiskSpaceCheck::Ok(available_mb)
            }
        }
        Err(e) => {
            warn!("Failed to check disk space: {e}");
            DiskSpaceCheck::Failed
        }
    }
}

/// The configured size limit in bytes, or `None` for no limit.
///
/// A zero from a hand edited config means the same thing as the field being
/// absent. Taken literally it would rotate on every container that arrived,
/// which spends the device on opening and closing files instead of recording.
fn rotation_bytes(mb: Option<u64>) -> Option<u64> {
    mb.filter(|mb| *mb > 0)
        .map(|mb| mb.saturating_mul(1024 * 1024))
}

/// The configured time limit, or `None` for no limit. Zero means no limit.
fn rotation_duration(minutes: Option<u64>) -> Option<Duration> {
    minutes
        .filter(|m| *m > 0)
        .map(|m| Duration::from_secs(m.saturating_mul(60)))
}

impl DiagTask {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ui_update_sender: Sender<display::DisplayState>,
        analysis_sender: Sender<AnalysisCtrlMessage>,
        analyzer_config: AnalyzerConfig,
        device: Device,
        notification_channel: tokio::sync::mpsc::Sender<Notification>,
        min_space_to_start_mb: u64,
        min_space_to_continue_mb: u64,
        max_recording_size_mb: Option<u64>,
        max_recording_minutes: Option<u64>,
        auto_delete_clean: bool,
        uploads_configured: bool,
        gps_mode: GpsMode,
        gps_fixed_coords: Option<(f64, f64)>,
        cell_tracker: Arc<RwLock<CellTracker>>,
    ) -> Self {
        Self {
            ui_update_sender,
            analysis_sender,
            analyzer_config,
            device,
            notification_channel,
            min_space_to_start_mb,
            min_space_to_continue_mb,
            auto_delete_clean,
            uploads_configured,
            max_recording_bytes: rotation_bytes(max_recording_size_mb),
            max_recording_duration: rotation_duration(max_recording_minutes),
            recording_started_at: None,
            gps_mode,
            gps_fixed_coords,
            cell_tracker,
            state: DiagState::Stopped,
            max_type_seen: EventType::Informational,
            bytes_since_space_check: 0,
            low_space_warned: false,
            latest_packet_timestamp: None,
        }
    }

    /// Start recording, and make sure the display agrees with the outcome.
    ///
    /// The display task starts life holding `DisplayState::Recording` and only
    /// ever changes it when a message arrives. `start_inner` sends that message
    /// as its last step, so every early return out of it — a disk too full to
    /// begin with, a failure creating the entry — used to leave the device
    /// showing a green "recording" line over a daemon that was recording
    /// nothing. Worst at boot, where nothing had ever set the state.
    ///
    /// The mid-recording case was already right: running out of space while
    /// recording calls `stop`, which sends `Paused`. This makes the "never
    /// started" case behave the same way, for every error path including ones
    /// added later.
    async fn start(&mut self, qmdl_store: &mut RecordingStore) -> Result<(), RecordingStoreError> {
        let result = self.start_inner(qmdl_store).await;
        if result.is_err()
            && let Err(e) = self
                .ui_update_sender
                .send(display::DisplayState::Paused)
                .await
        {
            warn!("couldn't send ui update message: {e}");
        }
        result
    }

    /// Start recording, returning an error if disk space is too low.
    async fn start_inner(
        &mut self,
        qmdl_store: &mut RecordingStore,
    ) -> Result<(), RecordingStoreError> {
        self.max_type_seen = EventType::Informational;
        self.bytes_since_space_check = 0;
        self.low_space_warned = false;

        // Making room has to happen here as well as in the write loop, and this
        // is the more important of the two. A device that filled up has already
        // stopped recording, so the check further down never runs again and it
        // could never recover on its own: exactly the situation the setting is
        // meant to prevent. Found by filling a device rather than by reading
        // the code.
        if self.auto_delete_clean
            && matches!(
                check_disk_space(
                    &qmdl_store.path,
                    self.min_space_to_start_mb,
                    self.min_space_to_continue_mb,
                ),
                DiskSpaceCheck::Critical(_) | DiskSpaceCheck::Warning(_)
            )
        {
            let freed = crate::cleanup::prune_clean_recordings(
                qmdl_store,
                self.min_space_to_start_mb,
                self.uploads_configured,
            )
            .await;
            if freed > 0 {
                info!("removed {freed} recording(s) that found nothing, to start recording");
            }
        }

        match check_disk_space(
            &qmdl_store.path,
            self.min_space_to_start_mb,
            self.min_space_to_continue_mb,
        ) {
            DiskSpaceCheck::Critical(mb) | DiskSpaceCheck::Warning(mb) => {
                return Err(RecordingStoreError::InsufficientDiskSpace(
                    mb,
                    self.min_space_to_start_mb,
                ));
            }
            DiskSpaceCheck::Ok(mb) => {
                info!("Starting recording with {}MB disk space available", mb);
            }
            DiskSpaceCheck::Failed => {}
        }

        // If a recording is somehow still running, finish it completely before
        // opening a new one. Finalizing writes the old recording's final size
        // and sends its RecordingFinished event while the store's current entry
        // still points at it. Creating the new entry first (as this used to do)
        // moved the current entry, so the old writer's final size landed on the
        // new recording. Rotation already finishes the old entry before calling
        // this, so there it is a no-op.
        if matches!(self.state, DiagState::Recording { .. }) {
            self.finish_current_entry(qmdl_store, None).await;
        }

        let (qmdl_gz_file, analysis_file) = qmdl_store.new_entry(self.gps_mode).await?;

        // For fixed-mode sessions, write the configured coordinates to the storage
        // immediately so the per-session GPS is stored durably and isn't affected
        // by future config changes or GPS API calls.
        if self.gps_mode == GpsMode::Fixed
            && let Some((lat, lon)) = self.gps_fixed_coords
            && let Some((entry_idx, _)) = qmdl_store.get_current_entry()
        {
            let mut gps_file = qmdl_store
                .open_entry_gps_for_append(entry_idx)
                .await?
                .ok_or(RecordingStoreError::GpsStorageNotFound)?;

            let record = GpsRecord {
                latest_packet_timestamp: None,
                system_time: rayhunter::clock::get_adjusted_now().timestamp(),
                lat,
                lon,
            };
            let mut json = serde_json::to_vec(&record)?;
            json.push(b'\n');
            gps_file
                .write_all(&json)
                .await
                .map_err(RecordingStoreError::WriteFileError)?;
        }
        let qmdl_writer = Box::new(QmdlWriter::new(qmdl_gz_file));
        let device_metadata = DeviceMetadata {
            home_plmn: rayhunter::sim::home_plmn(&self.device).await,
        };
        let analysis_writer =
            AnalysisWriter::new(analysis_file, &self.analyzer_config, &device_metadata)
                .await
                .map_err(RecordingStoreError::WriteFileError)?;
        self.state = DiagState::Recording {
            qmdl_writer,
            analysis_writer: Box::new(analysis_writer),
        };
        self.recording_started_at = Some(Instant::now());

        if let Err(e) = self
            .ui_update_sender
            .send(display::DisplayState::Recording)
            .await
        {
            warn!("couldn't send ui update message: {e}");
        }
        Ok(())
    }

    /// Close the running recording and queue it for analysis.
    ///
    /// Split out of `stop` so that rotating to a new recording can finish the
    /// old one without also telling the display that recording has paused.
    /// Sending Paused and then Recording a moment later reads on the device as
    /// a blink to the stopped colour, which on a detector is exactly the wrong
    /// thing to show when nothing has actually stopped.
    async fn finish_current_entry(
        &mut self,
        qmdl_store: &mut RecordingStore,
        reason: Option<String>,
    ) {
        self.stop_current_recording(qmdl_store).await;
        self.recording_started_at = None;
        if let Some(reason) = reason
            && let Err(e) = qmdl_store.set_current_stop_reason(reason).await
        {
            warn!("couldn't set stop reason: {e}");
        }
        if let Some((_, entry)) = qmdl_store.get_current_entry()
            && let Err(e) = self
                .analysis_sender
                .send(AnalysisCtrlMessage::RecordingFinished(
                    entry.name.to_string(),
                ))
                .await
        {
            warn!("couldn't send analysis message: {e}");
        }
        if let Err(e) = qmdl_store.close_current_entry().await {
            error!("couldn't close current entry: {e}");
        }
    }

    /// Stop recording, optionally annotating the entry with a reason.
    async fn stop(&mut self, qmdl_store: &mut RecordingStore, reason: Option<String>) {
        self.finish_current_entry(qmdl_store, reason).await;
        if let Err(e) = self
            .ui_update_sender
            .send(display::DisplayState::Paused)
            .await
        {
            warn!("couldn't send ui update message: {e}");
        }
    }

    /// Close the running recording and immediately open a new one.
    ///
    /// The highest severity seen is deliberately carried across. Rotation is
    /// the device's own decision, not the operator's, and letting it reset the
    /// display would mean an automatic housekeeping step could quietly clear a
    /// warning nobody had looked at yet. It stays until a recording is started
    /// by hand.
    async fn rotate(&mut self, qmdl_store: &mut RecordingStore, reason: String) {
        info!("{reason}");
        let carried = self.max_type_seen;
        self.finish_current_entry(qmdl_store, Some(reason)).await;

        if let Err(e) = self.start(qmdl_store).await {
            // Most likely the disk filled up. The old recording is safely
            // closed either way, so report it and settle in the stopped state
            // rather than pretending a recording is running.
            let reason = format!("couldn't start the next recording after rotating: {e}");
            error!("{reason}");
            self.notification_channel
                .send(Notification::new(
                    NotificationType::Warning,
                    reason.clone(),
                    None,
                ))
                .await
                .ok();
            self.stop(qmdl_store, Some(reason)).await;
            return;
        }

        self.max_type_seen = carried;
        if carried > EventType::Informational
            && let Err(e) = self
                .ui_update_sender
                .send(display::DisplayState::WarningDetected {
                    event_type: carried,
                })
                .await
        {
            warn!("couldn't send ui update message: {e}");
        }
    }

    async fn delete_entry(
        &mut self,
        qmdl_store: &mut RecordingStore,
        name: &str,
    ) -> Result<(), RecordingStoreError> {
        if qmdl_store.is_current_entry(name) {
            self.stop(qmdl_store, None).await;
        }
        let res = qmdl_store.delete_entry(name).await;
        if let Err(e) = res.as_ref() {
            error!("Error deleting QMDL entry {e}");
        }
        res
    }

    async fn delete_all_entries(
        &mut self,
        qmdl_store: &mut RecordingStore,
    ) -> Result<(), RecordingStoreError> {
        self.stop(qmdl_store, None).await;
        let res = qmdl_store.delete_all_entries().await;
        if let Err(e) = res.as_ref() {
            error!("Error deleting QMDL entries {e}");
        }
        res
    }

    async fn handle_gps_update(&mut self, qmdl_store: &RecordingStore, lat: f64, lon: f64) {
        let Some((entry_idx, _)) = qmdl_store.get_current_entry() else {
            info!("GPS update received but no recording active, not writing to storage");
            return;
        };
        let mut file = match qmdl_store.open_entry_gps_for_append(entry_idx).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                error!("GPS storage not found, cannot write GPS record");
                return;
            }
            Err(e) => {
                error!("failed to open GPS storage: {e}");
                return;
            }
        };
        let record = GpsRecord {
            latest_packet_timestamp: self.latest_packet_timestamp,
            system_time: rayhunter::clock::get_adjusted_now().timestamp(),
            lat,
            lon,
        };
        let Ok(mut json) = serde_json::to_vec(&record) else {
            error!("failed to serialize GPS record");
            return;
        };
        json.push(b'\n');
        if let Err(e) = file.write_all(&json).await {
            error!("failed to write GPS record to storage: {e}");
        }
    }

    async fn stop_current_recording(&mut self, qmdl_store: &mut RecordingStore) {
        let mut state = DiagState::Stopped;
        std::mem::swap(&mut self.state, &mut state);
        if let DiagState::Recording {
            qmdl_writer,
            analysis_writer,
            ..
        } = state
        {
            // Failing to close a writer is exactly what a failing or full flash
            // does, and it is not a reason to take the whole capture daemon
            // down: log it, record what we can, and settle into the stopped
            // state (already swapped in above) so recording can be started
            // again. Panicking here turned a recoverable storage fault into a
            // dead detector.
            let (qmdl_result, analysis_result) =
                (qmdl_writer.close().await, analysis_writer.close().await);
            match &qmdl_result {
                Ok(size) => {
                    if let Err(err) = qmdl_store.update_current_entry_qmdl_size(*size).await {
                        error!("failed to update QMDL entry size while closing it: {err:?}");
                    }
                }
                Err(err) => {
                    error!("failed to close QmdlWriter, recording may be incomplete: {err:?}")
                }
            }
            if let Err(err) = analysis_result {
                error!("failed to close AnalysisWriter, analysis may be incomplete: {err:?}");
            }
        }
    }

    async fn process_container(
        &mut self,
        qmdl_store: &mut RecordingStore,
        container: MessagesContainer,
    ) {
        self.process_container_inner(qmdl_store, container, false)
            .await
    }

    /// Process a container of synthetic demo messages. Identical to the real
    /// path, except every event it produces is labelled as a demo.
    async fn process_demo_container(
        &mut self,
        qmdl_store: &mut RecordingStore,
        container: MessagesContainer,
    ) {
        self.process_container_inner(qmdl_store, container, true)
            .await
    }

    async fn process_container_inner(
        &mut self,
        qmdl_store: &mut RecordingStore,
        container: MessagesContainer,
        demo: bool,
    ) {
        if container.data_type != DataType::UserSpace {
            debug!("skipping non-userspace diag messages...");
            return;
        }
        // Set when a size or time limit is reached. Rotating has to happen once
        // the borrow of `self.state` below has ended, since opening the next
        // recording replaces the writers this block is holding.
        let mut rotate_reason: Option<String> = None;

        // keep track of how many bytes were written to the QMDL file so we can read
        // a valid block of data from it in the HTTP server
        if let DiagState::Recording {
            qmdl_writer,
            analysis_writer,
        } = &mut self.state
        {
            if self.bytes_since_space_check >= DISK_CHECK_BYTES_INTERVAL {
                self.bytes_since_space_check = 0;
                match check_disk_space(
                    &qmdl_store.path,
                    self.min_space_to_start_mb,
                    self.min_space_to_continue_mb,
                ) {
                    DiskSpaceCheck::Critical(mb) => {
                        let reason = format!(
                            "Disk space critically low ({}MB free), recording stopped automatically",
                            mb
                        );
                        error!("{reason}");

                        self.notification_channel
                            .send(Notification::new(
                                NotificationType::Warning,
                                reason.clone(),
                                None,
                            ))
                            .await
                            .ok();

                        self.stop(qmdl_store, Some(reason)).await;
                        return;
                    }
                    DiskSpaceCheck::Warning(mb) => {
                        // Try to make room before telling anyone it is a
                        // problem, since if this works it is not one. Only
                        // touches recordings that were analysed and found
                        // nothing; see `cleanup`.
                        let freed = if self.auto_delete_clean {
                            crate::cleanup::prune_clean_recordings(
                                qmdl_store,
                                self.min_space_to_start_mb,
                                self.uploads_configured,
                            )
                            .await
                        } else {
                            0
                        };

                        if freed > 0 {
                            info!(
                                "removed {freed} recording(s) that found nothing, to keep recording"
                            );
                            // Space was reclaimed, so the next low reading is
                            // worth reporting afresh.
                            self.low_space_warned = false;
                        } else if !self.low_space_warned {
                            self.low_space_warned = true;
                            warn!("Disk space low: {}MB remaining", mb);
                            self.notification_channel
                                .send(Notification::new(
                                    NotificationType::Warning,
                                    format!("Disk space low: {}MB free", mb),
                                    Some(Duration::from_secs(30)),
                                ))
                                .await
                                .ok();
                        }
                    }
                    _ => {}
                }
            }

            if let Err(e) = qmdl_writer.write_container(&container).await {
                let reason = format!("failed to write to QMDL (disk full?): {e}");
                error!("{reason}");
                self.stop(qmdl_store, Some(reason)).await;
                return;
            }
            if let Ok(file_size) = qmdl_writer.size().await {
                debug!(
                    "total QMDL bytes written: {}, updating manifest...",
                    file_size
                );
                if let Err(e) = qmdl_store.update_current_entry_qmdl_size(file_size).await {
                    let reason = format!("failed to update manifest (disk full?): {e}");
                    error!("{reason}");
                    self.stop(qmdl_store, Some(reason)).await;
                    return;
                }
                debug!("done!");

                // Size limit. Checked against the file on disk rather than a
                // running total, so it means what it says on the manifest.
                if let Some(max) = self.max_recording_bytes
                    && file_size as u64 >= max
                {
                    rotate_reason = Some(format!(
                        "Started a new recording automatically: reached the {} MB limit",
                        max / 1024 / 1024
                    ));
                }
            }

            // Time limit. Second, so that a recording which hits both in the
            // same container is described by its size, which is the more
            // surprising of the two to arrive early.
            if rotate_reason.is_none()
                && let (Some(limit), Some(started)) =
                    (self.max_recording_duration, self.recording_started_at)
                && started.elapsed() >= limit
            {
                rotate_reason = Some(format!(
                    "Started a new recording automatically: reached the {} minute limit",
                    limit.as_secs() / 60
                ));
            }

            // Extract the latest packet timestamp from this container
            if let Some(ts) = container
                .messages()
                .into_iter()
                .filter_map(|r| match r {
                    Ok(Message::Log { timestamp, .. }) => Some(timestamp.to_datetime().timestamp()),
                    _ => None,
                })
                .max()
            {
                self.latest_packet_timestamp = Some(ts);
            }

            update_cell_info(&self.cell_tracker, &container).await;

            let container_bytes: usize = container.messages.iter().map(|m| m.data.len()).sum();
            self.bytes_since_space_check += container_bytes;
            let analysis = if demo {
                analysis_writer
                    .analyze_demo_container(container, crate::demo::DEMO_PREFIX)
                    .await
            } else {
                analysis_writer.analyze_container(container).await
            };
            let max_type = match analysis {
                Ok(t) => t,
                Err(e) => {
                    warn!("failed to analyze container: {e}");
                    EventType::Informational
                }
            };

            if max_type > EventType::Informational {
                info!("a heuristic triggered on this run!");
                // The notification worker is not essential to capture. If it has
                // gone away, log it and carry on analysing rather than panicking
                // the DIAG task at the exact moment a warning was found.
                if let Err(e) = self
                    .notification_channel
                    .send(Notification::new(
                        NotificationType::Warning,
                        format!("Rayhunter has detected a {:?} severity event", max_type),
                        Some(Duration::from_secs(60 * 5)),
                    ))
                    .await
                {
                    warn!("couldn't send notification, continuing to capture: {e}");
                }
            }

            if max_type > self.max_type_seen {
                self.max_type_seen = max_type;
                if self.max_type_seen > EventType::Informational
                    && let Err(e) = self
                        .ui_update_sender
                        .send(display::DisplayState::WarningDetected {
                            event_type: self.max_type_seen,
                        })
                        .await
                {
                    warn!("couldn't send ui update, continuing to capture: {e}");
                }
            }
        } else {
            debug!("no qmdl_writer set, continuing...");
        }

        if let Some(reason) = rotate_reason {
            self.rotate(qmdl_store, reason).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Pull the radio measurements out of a container and fold them into the
/// shared cell tracker.
///
/// These messages are decoded by the library already but nothing consumed
/// them, so they were being parsed and dropped. Reading them here keeps
/// this out of the analysis path, which stays focused on detection.
async fn update_cell_info(cell_tracker: &Arc<RwLock<CellTracker>>, container: &MessagesContainer) {
    use rayhunter::diag::diaglog::LogBody;

    let mut serving: Option<(u16, u32, SignalMeasurements, Option<u32>)> = None;
    let mut neighbors: Option<Vec<NeighborCell>> = None;
    let mut timing_advance: Option<u16> = None;
    let mut rrc_messages: Vec<Message> = Vec::new();
    let mut seen: u64 = 0;
    let mut skipped: u64 = 0;

    for message in container.messages() {
        seen += 1;
        if message.is_err() {
            skipped += 1;
        }
        let Ok(Message::Log {
            body,
            pending_msgs,
            outer_length,
            inner_length,
            log_type,
            timestamp,
        }) = message
        else {
            continue;
        };
        match body {
            LogBody::LteMl1ServingCellMeasurementAndEvaluation { data } => {
                serving = Some((
                    data.get_pci(),
                    data.get_earfcn(),
                    SignalMeasurements {
                        rsrp_dbm: data.get_meas_rsrp(),
                        rsrq_db: data.get_meas_rsrq(),
                        rssi_dbm: data.get_meas_rssi(),
                        avg_rsrp_dbm: Some(data.get_avg_rsrp()),
                        // The serving cell report carries no averaged quality.
                        avg_rsrq_db: None,
                    },
                    Some(data.get_s_search()),
                ));
            }
            LogBody::LteMl1NeighborCellsMeasurements { data } => {
                let earfcn = data.get_earfcn();
                neighbors = Some(
                    data.cells
                        .iter()
                        .map(|cell| NeighborCell {
                            pci: cell.pci,
                            earfcn,
                            signal: SignalMeasurements {
                                rsrp_dbm: cell.get_meas_rsrp(),
                                rsrq_db: cell.get_meas_rsrq(),
                                rssi_dbm: cell.get_meas_rssi(),
                                avg_rsrp_dbm: Some(cell.get_avg_rsrp()),
                                avg_rsrq_db: Some(cell.get_avg_rsrq()),
                            },
                            s_rxlev: Some(cell.get_s_rxlev()),
                        })
                        .collect(),
                );
            }
            LogBody::LteMacRachResponse { packet } => {
                // The random access response carries the timing advance, which
                // is the device's only read on how far away the tower is.
                for subpacket in &packet.subpackets {
                    if let rayhunter::diag::diaglog::mac::SubpacketBody::RachAttempt(attempt) =
                        &subpacket.body
                        && let Some(msg2) = attempt.get_msg2()
                    {
                        timing_advance = Some(msg2.ta);
                    }
                }
            }
            LogBody::LteRrcOtaMessage { .. } | LogBody::Nas4GMessage { .. } => {
                // Kept aside rather than decoded inline: these carry the cell
                // identity and the agreed security algorithms, and decoding is
                // the expensive part.
                rrc_messages.push(Message::Log {
                    pending_msgs,
                    outer_length,
                    inner_length,
                    log_type,
                    timestamp,
                    body,
                });
            }
            _ => {}
        }
    }

    if serving.is_none()
        && neighbors.is_none()
        && timing_advance.is_none()
        && rrc_messages.is_empty()
    {
        return;
    }

    let mut tracker = cell_tracker.write().await;
    tracker.record_messages(seen, skipped);
    if let Some((pci, earfcn, signal, search_threshold)) = serving {
        tracker.update_serving(pci, earfcn, signal, search_threshold);
    }
    if let Some(neighbors) = neighbors {
        tracker.update_neighbors(neighbors);
    }
    if let Some(ta) = timing_advance {
        tracker.update_timing_advance(ta);
    }

    // RRC is decoded for two things: the cell identity, which is only needed
    // until it is known, and the agreed cipher, which can change at any time.
    let need_identity = tracker
        .snapshot()
        .serving
        .is_some_and(|s| s.identity.is_none());
    for message in rrc_messages {
        let Ok(Some((_, gsmtap))) = rayhunter::gsmtap::parser::parse(message) else {
            continue;
        };
        let Ok(element) = InformationElement::try_from(&gsmtap) else {
            continue;
        };
        if need_identity && let Some(identity) = identity_from_information_element(&element) {
            tracker.update_identity(identity);
        }
        if let Some((cipher, integrity)) = rrc_security_from_information_element(&element) {
            tracker.update_rrc_security(cipher, integrity);
        }
        if let Some((cipher, integrity)) = nas_security_from_information_element(&element) {
            tracker.update_nas_security(cipher, integrity);
        }
        // Any NAS at all means this SIM is being served by the network's core,
        // which is the thing a dead or unaccepted SIM cannot produce. Recorded
        // before the identity read below, which only fires for the few
        // unciphered messages that carry one.
        if matches!(
            gsmtap.header.gsmtap_type,
            rayhunter::gsmtap::GsmtapType::LteNas(_)
        ) {
            tracker.update_nas_seen();
        }
        // Identities the device sends about itself. Read from the raw payload
        // rather than the parsed message, because the NAS parser generates the
        // identity field as an empty struct and discards its bytes.
        if gsmtap.header.gsmtap_type
            == rayhunter::gsmtap::GsmtapType::LteNas(rayhunter::gsmtap::LteNasSubtype::Plain)
            && let Some(identity) = crate::subscriber_id::identity_from_nas(&gsmtap.payload)
        {
            tracker.update_identity_sent(identity);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_diag_read_thread(
    task_tracker: &TaskTracker,
    device: Device,
    // Where the modem's diagnostic device lives; see Config::diag_device_path.
    diag_device_path: String,
    mut qmdl_file_rx: Receiver<DiagDeviceCtrlMessage>,
    qmdl_file_tx: Sender<DiagDeviceCtrlMessage>,
    ui_update_sender: Sender<display::DisplayState>,
    qmdl_store_lock: Arc<RwLock<RecordingStore>>,
    analysis_sender: Sender<AnalysisCtrlMessage>,
    analyzer_config: AnalyzerConfig,
    notification_channel: tokio::sync::mpsc::Sender<Notification>,
    min_space_to_start_mb: u64,
    min_space_to_continue_mb: u64,
    max_recording_size_mb: Option<u64>,
    max_recording_minutes: Option<u64>,
    auto_delete_clean: bool,
    uploads_configured: bool,
    gps_mode: GpsMode,
    gps_fixed_coords: Option<(f64, f64)>,
    cell_tracker: Arc<RwLock<CellTracker>>,
) {
    task_tracker.spawn(async move {
        info!("Using configuration for device: {0:?}", device);
        let mut dev = DiagDevice::new(&device, &diag_device_path)
            .await?;
        dev.config_logs()
            .await?;

        let mut diag_stream = pin!(dev.as_stream().into_stream());
        let mut diag_task = DiagTask::new(
            ui_update_sender,
            analysis_sender,
            analyzer_config,
            device.clone(),
            notification_channel,
            min_space_to_start_mb,
            min_space_to_continue_mb,
            max_recording_size_mb,
            max_recording_minutes,
            auto_delete_clean,
            uploads_configured,
            gps_mode,
            gps_fixed_coords,
            cell_tracker,
        );
        qmdl_file_tx
            .send(DiagDeviceCtrlMessage::StartRecording { response_tx: None })
            .await
            .unwrap();
        loop {
            tokio::select! {
                msg = qmdl_file_rx.recv() => {
                    match msg {
                        Some(DiagDeviceCtrlMessage::StartRecording { response_tx }) => {
                            let mut qmdl_store = qmdl_store_lock.write().await;
                            let result = diag_task.start(qmdl_store.deref_mut()).await;
                            if let Some(tx) = response_tx {
                                tx.send(result).ok();
                            }
                        },
                        Some(DiagDeviceCtrlMessage::StopRecording) => {
                            let mut qmdl_store = qmdl_store_lock.write().await;
                            diag_task.stop(qmdl_store.deref_mut(), None).await;
                        },
                        Some(DiagDeviceCtrlMessage::InjectDemo) => {
                            // Fed through exactly the path a real container
                            // takes, so it is written to the recording and
                            // analysed like anything off the air.
                            match crate::demo::demo_container() {
                                Some(container) => {
                                    info!("injecting a demo container (synthetic, clearly labelled)");
                                    let mut qmdl_store = qmdl_store_lock.write().await;
                                    diag_task.process_demo_container(qmdl_store.deref_mut(), container).await;
                                }
                                None => error!("failed to build the demo container"),
                            }
                        },
                        // None means all the Senders have been dropped, so it's
                        // time to go
                        Some(DiagDeviceCtrlMessage::Exit) | None => {
                            info!("Diag reader thread exiting...");
                            let mut qmdl_store = qmdl_store_lock.write().await;
                            diag_task.stop_current_recording(qmdl_store.deref_mut()).await;
                            return Ok(())
                        },
                        Some(DiagDeviceCtrlMessage::DeleteEntry { name, response_tx }) => {
                            let mut qmdl_store = qmdl_store_lock.write().await;
                            let resp = diag_task.delete_entry(qmdl_store.deref_mut(), name.as_str()).await;
                            if response_tx.send(resp).is_err() {
                                error!("Failed to send delete entry respons, receiver dropped");
                            }
                        },
                        Some(DiagDeviceCtrlMessage::DeleteAllEntries { response_tx }) => {
                            let mut qmdl_store = qmdl_store_lock.write().await;
                            let resp = diag_task.delete_all_entries(qmdl_store.deref_mut()).await;
                            if response_tx.send(resp).is_err() {
                                error!("Failed to send delete all entries respons, receiver dropped");
                            }
                        },
                        Some(DiagDeviceCtrlMessage::GpsUpdate { lat, lon }) => {
                            let qmdl_store = qmdl_store_lock.read().await;
                            diag_task.handle_gps_update(&qmdl_store, lat, lon).await;
                        },
                    }
                }
                maybe_container = diag_stream.next() => {
                    match maybe_container.unwrap() {
                        Ok(container) => {
                            let mut qmdl_store = qmdl_store_lock.write().await;
                            diag_task.process_container(qmdl_store.deref_mut(), container).await
                        },
                        Err(err) => {
                            error!("error reading diag device: {err}");
                            return Err(err);
                        }
                    }
                }
            }
        }
    });
}

/// Start recording API for web thread
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/start-recording",
    tag = "Recordings",
    responses(
        (status = StatusCode::ACCEPTED, description = "Success"),
        (status = StatusCode::FORBIDDEN, description = "System is in debug mode"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Recording action unsuccessful")
    ),
    summary = "Start recording",
    description = "Begin a new data capture."
))]
pub async fn start_recording(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if state.config.debug_mode {
        return Err((StatusCode::FORBIDDEN, "server is in debug mode".to_string()));
    }

    let (response_tx, response_rx) = oneshot::channel();
    state
        .diag_device_ctrl_sender
        .send(DiagDeviceCtrlMessage::StartRecording {
            response_tx: Some(response_tx),
        })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("couldn't send start recording message: {e}"),
            )
        })?;

    match response_rx.await {
        Ok(Ok(())) => Ok((StatusCode::ACCEPTED, "ok".to_string())),
        Ok(Err(reason)) => Err((StatusCode::INSUFFICIENT_STORAGE, reason.to_string())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to receive start recording response: {e}"),
        )),
    }
}

/// Stop recording API for web thread
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/stop-recording",
    tag = "Recordings",
    responses(
        (status = StatusCode::ACCEPTED, description = "Success"),
        (status = StatusCode::FORBIDDEN, description = "System is in debug mode"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Recording action unsuccessful")
    ),
    summary = "Stop recording",
    description = "Stop current data capture."
))]
pub async fn stop_recording(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if state.config.debug_mode {
        return Err((StatusCode::FORBIDDEN, "server is in debug mode".to_string()));
    }
    state
        .diag_device_ctrl_sender
        .send(DiagDeviceCtrlMessage::StopRecording)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("couldn't send stop recording message: {e}"),
            )
        })?;
    Ok((StatusCode::ACCEPTED, "ok".to_string()))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/delete-recording/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::ACCEPTED, description = "Success"),
        (status = StatusCode::FORBIDDEN, description = "System is in debug mode"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Delete action unsuccessful"),
        (status = StatusCode::BAD_REQUEST, description = "Bad recording name or no such recording")
    ),
    params(
        ("name" = String, Path, description = "QMDL file to delete")
    ),
    summary = "Delete recording",
    description = "Remove data capture file named {name}."
))]
pub async fn delete_recording(
    State(state): State<Arc<ServerState>>,
    Path(qmdl_name): Path<String>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if state.config.debug_mode {
        return Err((StatusCode::FORBIDDEN, "server is in debug mode".to_string()));
    }
    let (response_tx, response_rx) = oneshot::channel();
    state
        .diag_device_ctrl_sender
        .send(DiagDeviceCtrlMessage::DeleteEntry {
            name: qmdl_name.clone(),
            response_tx,
        })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("couldn't send delete entry message: {e}"),
            )
        })?;
    match response_rx.await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to receive delete response: {e}"),
        )
    })? {
        Ok(_) => Ok((StatusCode::ACCEPTED, "ok".to_string())),
        Err(RecordingStoreError::NoSuchEntryError) => Err((
            StatusCode::BAD_REQUEST,
            format!("no recording with name {qmdl_name}"),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("couldn't delete recording: {e}"),
        )),
    }
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/delete-all-recordings",
    tag = "Recordings",
    responses(
        (status = StatusCode::ACCEPTED, description = "Success"),
        (status = StatusCode::FORBIDDEN, description = "System is in debug mode"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Delete action unsuccessful")
    ),
    summary = "Delete all recordings",
    description = "Remove all saved data capture files."
))]
pub async fn delete_all_recordings(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if state.config.debug_mode {
        return Err((StatusCode::FORBIDDEN, "server is in debug mode".to_string()));
    }
    let (response_tx, response_rx) = oneshot::channel();
    state
        .diag_device_ctrl_sender
        .send(DiagDeviceCtrlMessage::DeleteAllEntries { response_tx })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("couldn't send delete all entries message: {e}"),
            )
        })?;
    match response_rx.await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to receive delete all response: {e}"),
        )
    })? {
        Ok(_) => Ok((StatusCode::ACCEPTED, "ok".to_string())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("couldn't delete recordings: {e}"),
        )),
    }
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/analysis-report/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::OK, description = "Success", body = ReportMetadata, content_type = "application/x-ndjson"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "No QMDL files available; start a new recording."),
        (status = StatusCode::NOT_FOUND, description = "File {name} not found")
    ),
    params(
        ("name" = String, Path, description = "QMDL file to analyze")
    ),
    summary = "Analysis report",
    description = "Download processed analysis report for QMDL file {name}, as well as the types (and versions) of analyzers used."
))]
pub async fn get_analysis_report(
    State(state): State<Arc<ServerState>>,
    Path(qmdl_name): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let qmdl_store = state.qmdl_store_lock.read().await;
    let (entry_index, _) = if qmdl_name == "live" {
        qmdl_store.get_current_entry().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "No QMDL data's being recorded to analyze, try starting a new recording!".to_string(),
        ))?
    } else {
        qmdl_store.entry_for_name(&qmdl_name).ok_or((
            StatusCode::NOT_FOUND,
            format!("Couldn't find QMDL entry with name \"{qmdl_name}\""),
        ))?
    };
    let analysis_file = qmdl_store
        .open_file(entry_index, FileKind::Analysis)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?
        .ok_or((StatusCode::NOT_FOUND, "Analysis file not found".to_string()))?;

    // Read and normalize the NDJSON file
    let reader = BufReader::new(analysis_file);
    let lines_stream = LinesStream::new(reader.lines());

    let mut normalizer = AnalysisLineNormalizer::new();
    let normalized_stream = lines_stream
        .try_filter(|line| future::ready(!line.is_empty()))
        .map_ok(move |line| normalizer.normalize_line(line));

    let headers = [(CONTENT_TYPE, "application/x-ndjson")];
    let body = Body::from_stream(normalized_stream);
    Ok((headers, body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero has to mean "no limit", not "rotate immediately".
    ///
    /// A config written by hand is the likely source of one, and taken
    /// literally it would close and reopen a recording for every container
    /// that arrived. The device would spend itself on file handling and stop
    /// keeping up with the radio, which is a detector that has stopped
    /// detecting.
    #[test]
    fn zero_limits_mean_no_rotation() {
        assert_eq!(rotation_bytes(Some(0)), None);
        assert_eq!(rotation_duration(Some(0)), None);
        assert_eq!(rotation_bytes(None), None);
        assert_eq!(rotation_duration(None), None);
    }

    #[test]
    fn limits_convert_to_bytes_and_seconds() {
        assert_eq!(rotation_bytes(Some(5)), Some(5 * 1024 * 1024));
        assert_eq!(rotation_duration(Some(15)), Some(Duration::from_secs(900)));
        assert_eq!(rotation_duration(Some(60)), Some(Duration::from_secs(3600)));
    }

    /// A nonsense value from a hand edited config must not wrap around into a
    /// small limit, which would rotate constantly instead of never.
    #[test]
    fn absurd_limits_saturate_rather_than_wrapping() {
        assert_eq!(rotation_bytes(Some(u64::MAX)), Some(u64::MAX));
        assert_eq!(
            rotation_duration(Some(u64::MAX)),
            Some(Duration::from_secs(u64::MAX))
        );
    }
}
