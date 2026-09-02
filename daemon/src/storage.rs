//! Where recordings go, and watching that it is still there.
//!
//! The hotspots have little flash of their own; a memory card has plenty, but
//! it can be pulled at any moment, or simply not be there when the device
//! boots. Rayhunter records to the card whenever it is present and usable,
//! falls back to internal storage while it is not, says so, and moves back
//! when the card returns. This module holds the decision, the checks it is
//! made from, and the task that keeps making it.
//!
//! Only the configured card path is ever mounted or written to by Rayhunter
//! itself. Everything else it does here is read-only: listing what the
//! system has mounted, so the settings page can offer real choices.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Local};
use log::{error, info, warn};
use serde::Serialize;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::diag::DiagDeviceCtrlMessage;
use crate::notifications::{Notification, NotificationType};
use crate::server::ServerState;
use crate::stats::DiskStats;

/// How often the card is checked. A pulled card is noticed within this.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Directories the supported devices mount a card on, checked when the user
/// has chosen nothing yet so the settings page can suggest them.
const KNOWN_CARD_DIRS: &[&str] = &["/media/card", "/media/sdcard", "/mnt/sdcard"];

/// What the config says about storage.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Internal storage: always there, always the fallback.
    pub internal: PathBuf,
    /// Where the card is mounted, when the user has chosen one.
    pub removable: Option<PathBuf>,
    /// The block device to mount there when the system has not. `None`
    /// looks for the first SD card partition.
    pub device: Option<String>,
}

impl StorageConfig {
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            internal: PathBuf::from(&config.qmdl_store_path),
            removable: config
                .removable_store_path
                .as_deref()
                .filter(|p| !p.trim().is_empty())
                .map(PathBuf::from),
            device: config
                .removable_store_device
                .clone()
                .filter(|d| !d.trim().is_empty()),
        }
    }
}

/// The card's condition, as far as the last check could tell.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum RemovableState {
    /// No card path is configured; internal storage only.
    NotConfigured,
    /// Mounted and writable.
    Present,
    /// Not mounted and nothing to mount.
    Missing,
    /// There, but cannot be used: not writable, or would not mount.
    Unusable { reason: String },
}

/// What the web interface is told about storage.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct StorageStatus {
    /// Where recordings are going right now.
    pub active_path: String,
    pub internal_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removable_path: Option<String>,
    pub removable: RemovableState,
    /// A card is configured but recordings are going to internal storage.
    pub using_fallback: bool,
    /// When the current arrangement began.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "apidocs", schema(value_type = Option<String>))]
    pub since: Option<DateTime<Local>>,
    /// The last change, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event: Option<String>,
}

impl StorageStatus {
    fn new(config: &StorageConfig, active: &Path, removable: RemovableState) -> Self {
        let using_fallback = config.removable.is_some() && active == config.internal;
        Self {
            active_path: active.to_string_lossy().into_owned(),
            internal_path: config.internal.to_string_lossy().into_owned(),
            removable_path: config
                .removable
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            removable,
            using_fallback,
            since: Some(Local::now()),
            last_event: None,
        }
    }
}

/// One line of `/proc/mounts`.
#[derive(Debug, Clone, PartialEq)]
pub struct MountEntry {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
}

/// Parse `/proc/mounts`. Paths with spaces come octal-escaped (`\040`), and
/// are unescaped here.
pub fn parse_mounts(contents: &str) -> Vec<MountEntry> {
    contents
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = fields.next()?;
            let mountpoint = fields.next()?;
            let fstype = fields.next()?;
            Some(MountEntry {
                device: unescape_mount_field(device),
                mountpoint: unescape_mount_field(mountpoint),
                fstype: fstype.to_string(),
            })
        })
        .collect()
}

fn unescape_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let value =
                (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            out.push(value);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The mount exactly at `path`, if any. Trailing slashes are ignored.
#[cfg(test)]
pub fn mount_at<'a>(path: &Path, mounts: &'a [MountEntry]) -> Option<&'a MountEntry> {
    let wanted = normalise(path);
    mounts
        .iter()
        .rev()
        .find(|m| normalise(Path::new(&m.mountpoint)) == wanted)
}

/// The mount a path lives on: the entry with the longest mountpoint that is
/// the path itself or one of its ancestors.
pub fn mount_covering<'a>(path: &Path, mounts: &'a [MountEntry]) -> Option<&'a MountEntry> {
    let wanted = normalise(path);
    mounts
        .iter()
        .filter(|m| {
            let point = normalise(Path::new(&m.mountpoint));
            point == "/" || wanted == point || wanted.starts_with(&format!("{point}/"))
        })
        .max_by_key(|m| normalise(Path::new(&m.mountpoint)).len())
}

/// Whether `mount` counts as the card for a configured path. A mount exactly
/// at the path always does: the user named it. An ancestor mount counts when
/// it looks like a card, sits on a known card directory, or is the device the
/// config names; otherwise every path would count as "mounted" on the root
/// filesystem.
fn mount_is_the_card(path: &Path, mount: &MountEntry, device_hint: Option<&str>) -> bool {
    let point = normalise(Path::new(&mount.mountpoint));
    if point == normalise(path) {
        return true;
    }
    // The root filesystem covers every path; it is never the card, whatever
    // disk it happens to be on.
    if point == "/" {
        return false;
    }
    looks_removable(mount)
        || KNOWN_CARD_DIRS
            .iter()
            .any(|d| normalise(Path::new(d)) == point)
        || device_hint.is_some_and(|d| d == mount.device)
}

/// Where to mount the card for a configured path: the known card directory
/// the path sits under, if any, else the path itself.
pub fn mount_target(path: &Path) -> PathBuf {
    let wanted = normalise(path);
    KNOWN_CARD_DIRS
        .iter()
        .map(|d| normalise(Path::new(d)))
        .find(|d| wanted == *d || wanted.starts_with(&format!("{d}/")))
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

fn normalise(path: &Path) -> String {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Whether a mount looks like a memory card or other plug-in storage rather
/// than part of the device itself.
pub fn looks_removable(entry: &MountEntry) -> bool {
    let device_ok = [
        "/dev/mmcblk",
        "/dev/sd",
        "/dev/block/mmcblk",
        "/dev/block/sd",
    ]
    .iter()
    .any(|prefix| entry.device.starts_with(prefix));
    let fstype_ok = matches!(
        entry.fstype.as_str(),
        "vfat" | "msdos" | "exfat" | "ext2" | "ext3" | "ext4" | "f2fs" | "ntfs" | "fuseblk"
    );
    device_ok && fstype_ok
}

/// The block device to mount, when the system has not mounted the card.
async fn card_device(hint: Option<&str>) -> Option<PathBuf> {
    if let Some(hint) = hint {
        let path = PathBuf::from(hint);
        return fs::metadata(&path).await.is_ok().then_some(path);
    }
    // The first partition of the first card, or the whole card when it has
    // no partition table.
    for n in 0..4 {
        for candidate in [format!("/dev/mmcblk{n}p1"), format!("/dev/mmcblk{n}")] {
            let path = PathBuf::from(candidate);
            if fs::metadata(&path).await.is_ok() {
                return Some(path);
            }
        }
    }
    None
}

/// Try to write a file at `path` and remove it again. A card that has been
/// pulled keeps its stale mount entry for a while; this is what tells the
/// difference.
pub async fn probe_writable(path: &Path) -> Result<(), String> {
    let probe = path.join(format!(".rayhunter-probe-{}", std::process::id()));
    fs::write(&probe, b"rayhunter")
        .await
        .map_err(|e| e.to_string())?;
    fs::remove_file(&probe).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Facts a check gathers; the decision is made from these so it can be
/// tested without a card.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub mounted: bool,
    pub writable: Result<(), String>,
    pub device_present: bool,
}

/// The decision, before any mounting is attempted.
pub fn judge(observation: &Observation) -> RemovableState {
    match (observation.mounted, &observation.writable) {
        (true, Ok(())) => RemovableState::Present,
        // A mount that will not take a write is either read-only, full, or a
        // card that has been pulled and left its entry behind.
        (true, Err(_)) if !observation.device_present => RemovableState::Missing,
        (true, Err(reason)) => RemovableState::Unusable {
            reason: reason.clone(),
        },
        (false, _) if observation.device_present => RemovableState::Unusable {
            reason: "card present but not mounted".to_string(),
        },
        (false, _) => RemovableState::Missing,
    }
}

async fn observe(path: &Path, device_hint: Option<&str>) -> (Observation, Option<PathBuf>) {
    let mounts = fs::read_to_string("/proc/mounts")
        .await
        .map(|c| parse_mounts(&c))
        .unwrap_or_default();
    let mounted_entry = mount_covering(path, &mounts)
        .filter(|m| mount_is_the_card(path, m, device_hint))
        .cloned();
    let device = match &mounted_entry {
        Some(entry) if entry.device.starts_with("/dev/") => {
            let dev = PathBuf::from(&entry.device);
            fs::metadata(&dev).await.is_ok().then_some(dev)
        }
        _ => card_device(device_hint).await,
    };
    let writable = if mounted_entry.is_some() {
        // The store may be a directory inside the card that does not exist
        // yet; creating it is part of finding out whether the card takes
        // writes.
        match fs::create_dir_all(path).await {
            Ok(()) => probe_writable(path).await,
            Err(e) => Err(e.to_string()),
        }
    } else {
        Err("not mounted".to_string())
    };
    (
        Observation {
            mounted: mounted_entry.is_some(),
            writable,
            device_present: device.is_some(),
        },
        device,
    )
}

/// Check the card, mounting it if the system has not, and unmounting a
/// stale entry a pulled card left behind so the next insertion can mount.
pub async fn check_removable(path: &Path, device_hint: Option<&str>) -> RemovableState {
    let (observation, device) = observe(path, device_hint).await;
    match judge(&observation) {
        RemovableState::Missing if observation.mounted => {
            // Pulled without unmounting. Detach the dead mount so a fresh
            // card can be mounted on the same path.
            let target = mount_target(path);
            info!("{}: card gone, detaching its stale mount", target.display());
            if let Err(e) = run_quiet("umount", &["-l", &target.to_string_lossy()]).await {
                warn!("could not detach {}: {e}", target.display());
            }
            RemovableState::Missing
        }
        RemovableState::Unusable { reason } if !observation.mounted => {
            let Some(device) = device else {
                return RemovableState::Unusable { reason };
            };
            let target = mount_target(path);
            info!("mounting {} on {}", device.display(), target.display());
            if let Err(e) = fs::create_dir_all(&target).await {
                return RemovableState::Unusable {
                    reason: format!("cannot create {}: {e}", target.display()),
                };
            }
            match run_quiet(
                "mount",
                &[&device.to_string_lossy(), &target.to_string_lossy()],
            )
            .await
            {
                Ok(()) => {
                    let (again, _) = observe(path, device_hint).await;
                    judge(&again)
                }
                Err(e) => RemovableState::Unusable {
                    reason: format!("mount failed: {e}"),
                },
            }
        }
        state => state,
    }
}

async fn run_quiet(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Where recordings should go right now: the card when it is usable, else
/// internal storage.
pub async fn choose_active(config: &StorageConfig) -> (PathBuf, RemovableState) {
    match &config.removable {
        None => (config.internal.clone(), RemovableState::NotConfigured),
        Some(removable) => {
            let state = check_removable(removable, config.device.as_deref()).await;
            if state == RemovableState::Present {
                (removable.clone(), state)
            } else {
                (config.internal.clone(), state)
            }
        }
    }
}

/// The status at startup, once the store has been opened at `active`.
pub fn initial_status(
    config: &StorageConfig,
    active: &Path,
    removable: RemovableState,
) -> StorageStatus {
    let mut status = StorageStatus::new(config, active, removable.clone());
    status.last_event = Some(match (&removable, status.using_fallback) {
        (RemovableState::NotConfigured, _) => "Recording to internal storage.".to_string(),
        (RemovableState::Present, _) => {
            format!("Recording to the memory card at {}.", status.active_path)
        }
        (RemovableState::Missing, _) => {
            "Memory card missing at startup: recording to internal storage until it returns."
                .to_string()
        }
        (RemovableState::Unusable { reason }, _) => {
            format!("Memory card cannot be used ({reason}): recording to internal storage.")
        }
    });
    status
}

/// A place the settings page can offer.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct StorageCandidate {
    pub path: String,
    /// `internal`, `card`, or `other` (a mount that looks removable but is
    /// not on a known card path).
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fstype: Option<String>,
    pub mounted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    /// Whether this is the configured card path.
    pub configured: bool,
}

/// What can be chosen: internal storage, every mounted thing that looks like
/// a card, the known card directories when a card device is present but not
/// mounted, and whatever is configured now, whatever its state.
pub async fn candidates(config: &StorageConfig) -> Vec<StorageCandidate> {
    let mounts = fs::read_to_string("/proc/mounts")
        .await
        .map(|c| parse_mounts(&c))
        .unwrap_or_default();
    let mut list = vec![describe(&config.internal, "internal", &mounts, false)];
    let mut seen: Vec<String> = vec![normalise(&config.internal)];
    // The configured path counts as choosing the mount it sits on, so a
    // recordings directory inside the card does not list the card twice.
    let configured_within = |path: &Path| {
        config
            .removable
            .as_ref()
            .is_some_and(|removable| path_is_within(removable, path))
    };
    let mut push = |path: &Path, kind: &str, list: &mut Vec<StorageCandidate>| {
        let key = normalise(path);
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        list.push(describe(path, kind, &mounts, configured_within(path)));
    };
    for entry in mounts.iter().filter(|m| looks_removable(m)) {
        let path = Path::new(&entry.mountpoint);
        let kind = if KNOWN_CARD_DIRS
            .iter()
            .any(|d| normalise(Path::new(d)) == normalise(path))
        {
            "card"
        } else {
            "other"
        };
        push(path, kind, &mut list);
    }
    if card_device(config.device.as_deref()).await.is_some() {
        for dir in KNOWN_CARD_DIRS {
            let path = Path::new(dir);
            if fs::metadata(path).await.is_ok() {
                push(path, "card", &mut list);
            }
        }
    }
    // The configured card, when nothing above covered it: absent, or on a
    // path of the user's own.
    if let Some(removable) = &config.removable
        && !list.iter().any(|c| c.configured)
    {
        push(removable, "card", &mut list);
    }
    list
}

/// Whether `path` is `root` itself or a directory inside it.
fn path_is_within(path: &Path, root: &Path) -> bool {
    let wanted = normalise(path);
    let here = normalise(root);
    wanted == here || wanted.starts_with(&format!("{here}/"))
}

fn describe(path: &Path, kind: &str, mounts: &[MountEntry], configured: bool) -> StorageCandidate {
    let entry = mount_covering(path, mounts).filter(|m| mount_is_the_card(path, m, None));
    // Sizes for a card that is not there would be those of whatever
    // filesystem its empty directory sits on, which is not what is asked.
    let stats = if kind == "internal" || entry.is_some() {
        DiskStats::new(&path.to_string_lossy()).ok()
    } else {
        None
    };
    StorageCandidate {
        path: path.to_string_lossy().into_owned(),
        kind: kind.to_string(),
        device: entry.map(|e| e.device.clone()),
        fstype: entry.map(|e| e.fstype.clone()),
        mounted: entry.is_some(),
        total_bytes: stats.as_ref().and_then(|s| s.total_bytes),
        available_bytes: stats.as_ref().and_then(|s| s.available_bytes),
        configured,
    }
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/storage/candidates",
    tag = "System",
    responses(
        (status = StatusCode::OK, description = "Places recordings can be stored", body = Vec<StorageCandidate>)
    ),
    summary = "Storage candidates",
    description = "Internal storage, every mounted memory card, and the configured card path with its current state, for the settings page's storage choice."
))]
pub async fn get_storage_candidates(
    State(state): State<Arc<ServerState>>,
) -> Json<Vec<StorageCandidate>> {
    let config = StorageConfig::from_config(&state.config);
    Json(candidates(&config).await)
}

/// Keep checking the card and move recordings whenever its state changes.
pub fn run_storage_monitor(
    task_tracker: &TaskTracker,
    config: StorageConfig,
    status: Arc<RwLock<StorageStatus>>,
    diag_tx: mpsc::Sender<DiagDeviceCtrlMessage>,
    notifications: mpsc::Sender<Notification>,
    shutdown_token: CancellationToken,
) {
    let Some(removable) = config.removable.clone() else {
        return;
    };
    task_tracker.spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => return,
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
            }
            let state = check_removable(&removable, config.device.as_deref()).await;
            let (previous_state, active_is_card) = {
                let status = status.read().await;
                (
                    status.removable.clone(),
                    Path::new(&status.active_path) == removable,
                )
            };
            let want_card = state == RemovableState::Present;
            if want_card == active_is_card {
                if state != previous_state {
                    let mut status = status.write().await;
                    status.removable = state;
                }
                continue;
            }
            let (target, reason, event) = if want_card {
                (
                    removable.clone(),
                    "memory card returned".to_string(),
                    format!(
                        "Memory card back: recording to it again at {}.",
                        removable.display()
                    ),
                )
            } else {
                let free = DiskStats::new(&config.internal.to_string_lossy())
                    .ok()
                    .and_then(|d| d.available_bytes)
                    .map(|b| format!(" ({} MB free)", b / 1024 / 1024))
                    .unwrap_or_default();
                let why = match &state {
                    RemovableState::Unusable { reason } => {
                        format!("Memory card cannot be used ({reason})")
                    }
                    _ => "Memory card missing".to_string(),
                };
                (
                    config.internal.clone(),
                    why.clone(),
                    format!("{why}: recording to internal storage{free} until it returns."),
                )
            };
            let (response_tx, response_rx) = oneshot::channel();
            if diag_tx
                .send(DiagDeviceCtrlMessage::SwitchStore {
                    path: target.clone(),
                    reason: reason.clone(),
                    response_tx,
                })
                .await
                .is_err()
            {
                return;
            }
            match response_rx.await {
                Ok(Ok(())) => {
                    warn!("{event}");
                    {
                        let mut status = status.write().await;
                        *status = StorageStatus::new(&config, &target, state);
                        status.last_event = Some(event.clone());
                    }
                    let _ = notifications
                        .send(Notification::new(NotificationType::Storage, event, None))
                        .await;
                }
                Ok(Err(e)) => {
                    error!("could not move recordings to {}: {e}", target.display());
                    let mut status = status.write().await;
                    status.removable = state;
                    status.last_event = Some(format!(
                        "Could not move recordings to {}: {e}",
                        target.display()
                    ));
                }
                Err(_) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORBIC_MOUNTS: &str = "rootfs / rootfs rw 0 0\n\
        ubi0:usrfs /data ubifs rw,relatime 0 0\n\
        /dev/mmcblk0p1 /media/card vfat rw,relatime,fmask=0022 0 0\n\
        /dev/sda1 /mnt/my\\040disk ext4 rw 0 0\n\
        tmpfs /tmp tmpfs rw 0 0\n";

    #[test]
    fn mounts_parse_with_escapes() {
        let mounts = parse_mounts(ORBIC_MOUNTS);
        assert_eq!(mounts.len(), 5);
        assert_eq!(mounts[2].device, "/dev/mmcblk0p1");
        assert_eq!(mounts[2].fstype, "vfat");
        assert_eq!(mounts[3].mountpoint, "/mnt/my disk");
        assert!(parse_mounts("garbage\n").is_empty());
    }

    #[test]
    fn mount_lookup_ignores_trailing_slashes() {
        let mounts = parse_mounts(ORBIC_MOUNTS);
        assert!(mount_at(Path::new("/media/card/"), &mounts).is_some());
        assert!(mount_at(Path::new("/media/card"), &mounts).is_some());
        assert!(mount_at(Path::new("/media"), &mounts).is_none());
        assert!(mount_at(Path::new("/data/rayhunter/qmdl"), &mounts).is_none());
        assert_eq!(mount_at(Path::new("/"), &mounts).unwrap().fstype, "rootfs");
    }

    #[test]
    fn a_path_inside_the_card_is_covered_by_its_mount() {
        let mounts = parse_mounts(ORBIC_MOUNTS);
        let card = mount_covering(Path::new("/media/card/qmdl"), &mounts).unwrap();
        assert_eq!(card.mountpoint, "/media/card");
        assert!(mount_is_the_card(Path::new("/media/card/qmdl"), card, None));
        // Internal storage is covered by the /data mount, which is not a card.
        let data = mount_covering(Path::new("/data/rayhunter/qmdl"), &mounts).unwrap();
        assert_eq!(data.mountpoint, "/data");
        assert!(!mount_is_the_card(
            Path::new("/data/rayhunter/qmdl"),
            data,
            None
        ));
        // Naming the device makes any mount of it the card.
        assert!(mount_is_the_card(
            Path::new("/data/rayhunter/qmdl"),
            data,
            Some("ubi0:usrfs")
        ));
        // Anything else lands on the root filesystem, which never counts.
        let root = mount_covering(Path::new("/opt/cards"), &mounts).unwrap();
        assert_eq!(root.mountpoint, "/");
        assert!(!mount_is_the_card(Path::new("/opt/cards"), root, None));
        // A mount exactly at the configured path counts whatever it is.
        let tmp = mount_covering(Path::new("/tmp"), &mounts).unwrap();
        assert!(mount_is_the_card(Path::new("/tmp"), tmp, None));
    }

    #[test]
    fn the_mount_target_is_the_known_card_directory_when_there_is_one() {
        assert_eq!(
            mount_target(Path::new("/media/card/qmdl")),
            PathBuf::from("/media/card")
        );
        assert_eq!(
            mount_target(Path::new("/media/sdcard")),
            PathBuf::from("/media/sdcard")
        );
        assert_eq!(
            mount_target(Path::new("/data/fakecard")),
            PathBuf::from("/data/fakecard")
        );
    }

    #[test]
    fn removable_means_card_like_device_on_a_plain_filesystem() {
        let mounts = parse_mounts(ORBIC_MOUNTS);
        assert!(looks_removable(&mounts[2]));
        assert!(looks_removable(&mounts[3]));
        assert!(!looks_removable(&mounts[1]), "internal ubifs");
        assert!(!looks_removable(&mounts[4]), "tmpfs");
    }

    fn observation(mounted: bool, writable: Result<(), &str>, device_present: bool) -> Observation {
        Observation {
            mounted,
            writable: writable.map_err(|e| e.to_string()),
            device_present,
        }
    }

    #[test]
    fn the_decision_covers_every_case() {
        assert_eq!(
            judge(&observation(true, Ok(()), true)),
            RemovableState::Present
        );
        // Mounted and writable even though the device node cannot be found:
        // still usable.
        assert_eq!(
            judge(&observation(true, Ok(()), false)),
            RemovableState::Present
        );
        // Pulled card: stale mount, write fails, device gone.
        assert_eq!(
            judge(&observation(true, Err("EIO"), false)),
            RemovableState::Missing
        );
        assert_eq!(
            judge(&observation(true, Err("read-only"), true)),
            RemovableState::Unusable {
                reason: "read-only".into()
            }
        );
        assert!(matches!(
            judge(&observation(false, Err("not mounted"), true)),
            RemovableState::Unusable { .. }
        ));
        assert_eq!(
            judge(&observation(false, Err("not mounted"), false)),
            RemovableState::Missing
        );
    }

    #[tokio::test]
    async fn probe_writes_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        probe_writable(dir.path()).await.unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(
            probe_writable(Path::new("/nonexistent/rayhunter"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_unmounted_directory_with_no_card_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        // A device hint that does not exist means no card to mount.
        let state = check_removable(dir.path(), Some("/dev/rayhunter-no-such-card")).await;
        assert_eq!(state, RemovableState::Missing);
        let config = StorageConfig {
            internal: PathBuf::from("/data/rayhunter/qmdl"),
            removable: Some(dir.path().to_path_buf()),
            device: Some("/dev/rayhunter-no-such-card".into()),
        };
        let (active, state) = choose_active(&config).await;
        assert_eq!(active, config.internal);
        assert_eq!(state, RemovableState::Missing);
        let status = initial_status(&config, &active, state);
        assert!(status.using_fallback);
        assert!(status.last_event.unwrap().contains("missing"));
    }

    #[tokio::test]
    async fn no_card_configured_means_internal_only() {
        let config = StorageConfig {
            internal: PathBuf::from("/data/rayhunter/qmdl"),
            removable: None,
            device: None,
        };
        let (active, state) = choose_active(&config).await;
        assert_eq!(active, config.internal);
        assert_eq!(state, RemovableState::NotConfigured);
        let status = initial_status(&config, &active, state);
        assert!(!status.using_fallback);
        assert!(status.removable_path.is_none());
    }

    #[tokio::test]
    async fn candidates_start_with_internal_and_include_the_configured_card() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            internal: dir.path().to_path_buf(),
            removable: Some(PathBuf::from("/media/card")),
            device: Some("/dev/rayhunter-no-such-card".into()),
        };
        let list = candidates(&config).await;
        assert_eq!(list[0].kind, "internal");
        assert_eq!(list[0].path, dir.path().to_string_lossy());
        assert!(list[0].total_bytes.is_some());
        let card = list
            .iter()
            .find(|c| c.path == "/media/card")
            .expect("configured card listed");
        assert!(card.configured);
        assert_eq!(card.kind, "card");
        // An absent card reports no sizes rather than its parent's.
        assert!(!card.mounted);
        assert!(card.total_bytes.is_none());
        // Nothing is listed twice.
        let mut paths: Vec<_> = list.iter().map(|c| c.path.clone()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), list.len());
    }

    #[test]
    fn a_configured_directory_inside_a_mount_belongs_to_it() {
        let inside = Path::new("/media/card/qmdl");
        assert!(path_is_within(inside, Path::new("/media/card")));
        assert!(path_is_within(inside, Path::new("/media/card/qmdl")));
        // Inside /media too, as far as the path goes; that /media is not a
        // card is a question for the mount list, not for this.
        assert!(path_is_within(inside, Path::new("/media")));
        assert!(!path_is_within(inside, Path::new("/mnt/sdcard")));
        assert!(!path_is_within(
            Path::new("/media/cardx"),
            Path::new("/media/card")
        ));
    }
}
