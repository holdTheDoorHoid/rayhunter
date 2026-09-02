//! The per-recording sidecar: gathered when a recording starts, completed
//! when it closes, and served to the web interface.
//!
//! The shape of the file is [`rayhunter::recording_metadata`]; this module is
//! the part that knows how to fill it in on a running device.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use chrono::Local;
use log::{info, warn};
use rayhunter::recording_metadata::{
    ClockInfo, HardwareInfo, RecordingSidecar, ResourceInfo, SIDECAR_VERSION, SoftwareInfo,
    WifiInfo,
};
use tokio::fs;
use tokio::sync::RwLock;
use wifi_station::{WifiState, WifiStatus};

use crate::qmdl_store::FileKind;
use crate::server::ServerState;
use crate::stats::DiskStats;
use rayhunter::util::RuntimeMetadata;

/// The WiFi client's state, as the sidecar records it.
pub async fn wifi_info(enabled: bool, status: &Arc<RwLock<WifiStatus>>) -> WifiInfo {
    let status = status.read().await;
    let state = match status.state {
        WifiState::Disabled => "disabled",
        WifiState::Connecting => "connecting",
        WifiState::Connected => "connected",
        WifiState::Failed => "failed",
        WifiState::Recovering => "recovering",
        WifiState::DataPathDead => "data path dead",
    };
    WifiInfo {
        client_enabled: enabled,
        client_state: state.to_string(),
        connected_network: status.ssid.clone(),
    }
}

/// Everything the sidecar records at the start of a recording.
pub async fn collect(
    recording: &str,
    hardware: &HardwareInfo,
    home_plmn: BTreeSet<String>,
    wifi: Option<WifiInfo>,
    storage_path: &Path,
) -> RecordingSidecar {
    let runtime = RuntimeMetadata::new();
    let software = SoftwareInfo {
        rayhunter_version: runtime.rayhunter_version,
        system_os: runtime.system_os,
        arch: runtime.arch,
        kernel: first_line("/proc/version").await,
    };
    let offset = rayhunter::clock::get_offset().num_seconds();
    let clock = ClockInfo {
        system_time_at_start: Some(Local::now()),
        offset_seconds_at_start: offset,
        corrected_time_at_start: Some(rayhunter::clock::get_adjusted_now()),
        uptime_seconds_at_start: uptime_seconds().await,
        ..Default::default()
    };
    let disk = DiskStats::new(&storage_path.to_string_lossy()).ok();
    let (memory_total_kb, memory_available_kb) = meminfo().await;
    let resources = ResourceInfo {
        storage_path: Some(storage_path.to_string_lossy().into_owned()),
        disk_total_bytes: disk.as_ref().and_then(|d| d.total_bytes),
        disk_available_bytes: disk.as_ref().and_then(|d| d.available_bytes),
        memory_total_kb,
        memory_available_kb,
    };
    RecordingSidecar {
        sidecar_version: SIDECAR_VERSION,
        recording: recording.to_string(),
        software,
        hardware: hardware.clone(),
        home_plmn,
        clock,
        resources,
        wifi,
        redacted_fields: Vec::new(),
    }
}

/// Write the sidecar, replacing any earlier one atomically so a reader never
/// sees a half-written file.
pub async fn write(path: &Path, sidecar: &RecordingSidecar) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(sidecar).map_err(std::io::Error::other)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, json).await?;
    fs::rename(&tmp, path).await
}

/// Read a sidecar. `None` when there is none, or when it cannot be read,
/// which is logged: a recording without one is simply older than the
/// sidecar, and analysis carries on with what it can find out live.
pub async fn read(path: &Path) -> Option<RecordingSidecar> {
    match fs::read(path).await {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(sidecar) => Some(sidecar),
            Err(e) => {
                warn!(
                    "ignoring unreadable recording metadata {}: {e}",
                    path.display()
                );
                None
            }
        },
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => {
            warn!("couldn't read recording metadata {}: {e}", path.display());
            None
        }
    }
}

/// Add the closing clock readings to a sidecar. A recording with no sidecar
/// is left alone.
pub async fn finish(path: &Path) -> std::io::Result<()> {
    let Some(mut sidecar) = read(path).await else {
        return Ok(());
    };
    let offset = rayhunter::clock::get_offset().num_seconds();
    sidecar.clock.system_time_at_end = Some(Local::now());
    sidecar.clock.offset_seconds_at_end = Some(offset);
    sidecar.clock.uptime_seconds_at_end = uptime_seconds().await;
    sidecar.clock.offset_changed_during_recording =
        Some(offset != sidecar.clock.offset_seconds_at_start);
    write(path, &sidecar).await
}

async fn first_line(path: &str) -> Option<String> {
    let contents = fs::read_to_string(path).await.ok()?;
    let line = contents.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Seconds since boot, the first field of `/proc/uptime`.
async fn uptime_seconds() -> Option<f64> {
    let contents = fs::read_to_string("/proc/uptime").await.ok()?;
    parse_uptime(&contents)
}

fn parse_uptime(contents: &str) -> Option<f64> {
    contents.split_whitespace().next()?.parse().ok()
}

/// Total and available memory in kB from `/proc/meminfo`.
async fn meminfo() -> (Option<u64>, Option<u64>) {
    match fs::read_to_string("/proc/meminfo").await {
        Ok(contents) => parse_meminfo(&contents),
        Err(_) => (None, None),
    }
}

fn parse_meminfo(contents: &str) -> (Option<u64>, Option<u64>) {
    let field = |name: &str| {
        contents.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim() != name {
                return None;
            }
            value.split_whitespace().next()?.parse::<u64>().ok()
        })
    };
    (field("MemTotal"), field("MemAvailable"))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/recording-metadata/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::OK, description = "The device details saved with the recording", content_type = "application/json"),
        (status = StatusCode::NOT_FOUND, description = "No such recording, or it was made before device details were saved")
    ),
    params(
        ("name" = String, Path, description = "The recording's name")
    ),
    summary = "Recording metadata",
    description = "What Rayhunter knew about the device when the recording {name} was made: the hardware and software, the home network, the clock and its correction, storage and memory, and the WiFi client. The same file is included in the recording's zip; the shareable zip carries a copy with the home network and WiFi details removed."
))]
pub async fn get_recording_metadata(
    State(state): State<Arc<ServerState>>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<RecordingSidecar>, (StatusCode, String)> {
    let path = {
        let qmdl_store = state.qmdl_store_lock.read().await;
        let (_, entry) = qmdl_store.entry_for_name(&name).ok_or((
            StatusCode::NOT_FOUND,
            format!("couldn't find entry with name {name}"),
        ))?;
        FileKind::Meta.get_filepath(&entry.name, &qmdl_store.path, entry.compressed)
    };
    match read(&path).await {
        Some(sidecar) => Ok(Json(sidecar)),
        None => {
            info!("no recording metadata for {name}");
            Err((
                StatusCode::NOT_FOUND,
                format!("no device details were saved with recording {name}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_and_meminfo_parse() {
        assert_eq!(parse_uptime("3219.86 3106.84\n"), Some(3219.86));
        assert_eq!(parse_uptime(""), None);
        let meminfo = "MemTotal:         163612 kB\nMemFree:           22576 kB\n\
                       MemAvailable:      98880 kB\n";
        assert_eq!(parse_meminfo(meminfo), (Some(163612), Some(98880)));
        assert_eq!(parse_meminfo("garbage"), (None, None));
    }

    #[tokio::test]
    async fn sidecar_is_written_finished_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1700000000-0-meta.json");
        // Finishing a recording that has no sidecar is not an error.
        finish(&path).await.unwrap();
        assert!(read(&path).await.is_none());

        let sidecar = collect(
            "1700000000-0",
            &HardwareInfo {
                device: "orbic".into(),
                ..Default::default()
            },
            ["311-480".to_string()].into_iter().collect(),
            None,
            dir.path(),
        )
        .await;
        assert_eq!(sidecar.sidecar_version, SIDECAR_VERSION);
        assert!(sidecar.clock.system_time_at_start.is_some());
        assert!(sidecar.clock.system_time_at_end.is_none());
        assert!(sidecar.resources.disk_available_bytes.is_some());
        assert!(!sidecar.software.rayhunter_version.is_empty());

        write(&path, &sidecar).await.unwrap();
        assert!(!dir.path().join("1700000000-0-meta.json.tmp").exists());
        assert_eq!(read(&path).await.unwrap(), sidecar);

        finish(&path).await.unwrap();
        let finished = read(&path).await.unwrap();
        assert!(finished.clock.system_time_at_end.is_some());
        assert_eq!(finished.clock.offset_changed_during_recording, Some(false));
        assert_eq!(finished.home_plmn, sidecar.home_plmn);
    }

    #[tokio::test]
    async fn an_unreadable_sidecar_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x-meta.json");
        fs::write(&path, b"not json").await.unwrap();
        assert!(read(&path).await.is_none());
    }
}
