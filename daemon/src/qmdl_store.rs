use std::fmt::Display;
use std::io::{self, ErrorKind};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Distinguishes recordings created within the same clock second. Monotonic for
/// the life of the process, so two recordings started in the same second get
/// different names; combined with exclusive file creation in `new_entry`, a
/// name collision can never be resolved by truncating existing evidence.
static ENTRY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

use crate::config::GpsMode;
use chrono::{DateTime, Local, TimeDelta};
use log::{error, info, warn};
use rayhunter::util::RuntimeMetadata;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    fs::{self, File, OpenOptions},
    io::AsyncWriteExt,
};

#[derive(Debug, Error)]
pub enum RecordingStoreError {
    #[error("Can't close an entry when there's no current entry")]
    NoCurrentEntry,
    #[error("An entry with that name doesn't exist")]
    NoSuchEntryError,
    #[error("Couldn't create file: {0}")]
    CreateFileError(tokio::io::Error),
    #[error("Couldn't read file: {0}")]
    ReadFileError(tokio::io::Error),
    #[error("Couldn't write file: {0}")]
    WriteFileError(tokio::io::Error),
    #[error("Couldn't delete file: {0}")]
    DeleteFileError(tokio::io::Error),
    #[error("Couldn't open directory at path: {0}")]
    OpenDirError(tokio::io::Error),
    #[error("Couldn't read manifest file: {0}")]
    ReadManifestError(tokio::io::Error),
    #[error("Couldn't write manifest file: {0}")]
    WriteManifestError(tokio::io::Error),
    #[error("Couldn't parse QMDL store manifest file: {0}")]
    ParseManifestError(toml::de::Error),
    #[error("Insufficient disk space: {0}MB available, {1}MB required")]
    InsufficientDiskSpace(u64, u64),
    #[error("GPS storage directory not found")]
    GpsStorageNotFound,
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Qmdl,
    Analysis,
    Gps,
    /// The device details saved beside the recording, see
    /// [`rayhunter::recording_metadata`].
    Meta,
}

impl FileKind {
    // List of all possible physical files on disk.
    pub const ALL: &'static [FileKind] = &[
        FileKind::Qmdl,
        FileKind::Analysis,
        FileKind::Gps,
        FileKind::Meta,
    ];

    pub fn get_filename(&self, entry_name: &str, qmdl_compressed: bool) -> String {
        match self {
            FileKind::Qmdl if qmdl_compressed => format!("{}.qmdl.gz", entry_name),
            FileKind::Qmdl => format!("{}.qmdl", entry_name),
            FileKind::Analysis => format!("{}.ndjson", entry_name),
            FileKind::Gps => format!("{}-gps.ndjson", entry_name),
            FileKind::Meta => rayhunter::recording_metadata::sidecar_filename(entry_name),
        }
    }

    pub fn get_filepath<P: AsRef<Path>>(
        &self,
        entry_name: &str,
        base_path: P,
        qmdl_compressed: bool,
    ) -> PathBuf {
        base_path
            .as_ref()
            .join(self.get_filename(entry_name, qmdl_compressed))
    }
}

impl Display for FileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileKind::Qmdl => write!(f, "QMDL"),
            FileKind::Analysis => write!(f, "analysis"),
            FileKind::Gps => write!(f, "GPS"),
            FileKind::Meta => write!(f, "metadata"),
        }
    }
}

pub struct RecordingStore {
    pub path: PathBuf,
    pub manifest: Manifest,
    pub current_entry: Option<usize>, // index into manifest
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Debug)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
}

/// The structure of an entry in the QMDL manifest table
#[derive(Deserialize, Serialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ManifestEntry {
    /// The name of the entry
    pub name: String,
    /// The system time when recording began
    #[cfg_attr(feature = "apidocs", schema(value_type = String))]
    pub start_time: DateTime<Local>,
    /// The system time when the last message was recorded to the file
    #[cfg_attr(feature = "apidocs", schema(value_type = String))]
    pub last_message_time: Option<DateTime<Local>>,
    pub qmdl_size_bytes: usize,
    /// The rayhunter daemon version which generated the file
    pub rayhunter_version: Option<String>,
    /// The OS which created the file
    pub system_os: Option<String>,
    /// The architecture on which the OS was running
    pub arch: Option<String>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// When the manifest was uploaded to a WebDAV server
    #[cfg_attr(feature = "apidocs", schema(value_type = String))]
    pub upload_time: Option<DateTime<Local>>,
    #[serde(default)]
    pub gps_mode: Option<GpsMode>,
    #[serde(default)]
    pub compressed: bool,
    /// A name chosen by the person recording, shown instead of the timestamp.
    ///
    /// Recordings are named by the second they started, which says nothing
    /// about why anyone made them. Kept in the manifest rather than inside the
    /// capture so that renaming never rewrites a recording, which is evidence.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Free text about the circumstances of the recording.
    #[serde(default)]
    pub notes: Option<String>,
    /// What happened when this recording was contributed to a community
    /// dataset, if it was. See `telemetry/DESIGN.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_submission: Option<TelemetrySubmission>,
    /// The owner asked for this recording never to be contributed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub telemetry_excluded: bool,
}

/// The record of one recording having been contributed: enough to show on
/// the history page, and to withdraw it later.
#[derive(Deserialize, Serialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct TelemetrySubmission {
    pub submission_id: String,
    pub tier: telemetry_format::manifest::Tier,
    #[cfg_attr(feature = "apidocs", schema(value_type = String))]
    pub submitted_at: DateTime<Local>,
    /// Which of the unit's signing keys signed it, so a withdrawal can be
    /// signed by the same one after rotation.
    pub key_id: String,
    pub server_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "apidocs", schema(value_type = Option<String>))]
    pub withdrawn_at: Option<DateTime<Local>>,
}

/// Longest display name accepted, matching the length in EFForg/rayhunter#501.
pub const MAX_DISPLAY_NAME: usize = 29;

/// Longest note accepted. Room for the circumstances of a recording without
/// letting the manifest, which is read whole on every poll, grow without limit.
pub const MAX_NOTES: usize = 2000;

/// Reduce a display name to something safe to put in a filename.
///
/// The name reaches the outside world as the name of a downloaded zip, so it
/// has to survive being written to any filesystem and must not be able to
/// steer a path. Everything outside letters, digits, dash and underscore is
/// replaced, which follows the `\w` the issue asked for while also ruling out
/// separators, leading dots and the device's own reserved names.
pub fn sanitize_display_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(MAX_DISPLAY_NAME)
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.to_string()
    }
}

impl ManifestEntry {
    fn new(gps_mode: GpsMode) -> Self {
        let now = rayhunter::clock::get_adjusted_now();
        let metadata = RuntimeMetadata::new();
        // "<unix-seconds>-<sequence>". The seconds keep the name human-readable
        // and chronological; the sequence makes it unique even when two
        // recordings start in the same second. Recovery parses the seconds back
        // off the front, and still accepts the old bare-seconds names.
        let sequence = ENTRY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        ManifestEntry {
            name: format!("{}-{}", now.timestamp(), sequence),
            start_time: now,
            last_message_time: None,
            qmdl_size_bytes: 0,
            rayhunter_version: Some(metadata.rayhunter_version),
            system_os: Some(metadata.system_os),
            arch: Some(metadata.arch),
            stop_reason: None,
            upload_time: None,
            gps_mode: Some(gps_mode),
            compressed: true,
            display_name: None,
            notes: None,
            telemetry_submission: None,
            telemetry_excluded: false,
        }
    }

    pub fn get_filepath<P: AsRef<Path>>(&self, file_kind: FileKind, path: P) -> PathBuf {
        file_kind.get_filepath(&self.name, path, self.compressed)
    }
}

impl RecordingStore {
    // Loads an existing RecordingStore at the given path. Errors if no store exists,
    // or if it's malformed.
    pub async fn load<P>(path: P) -> Result<Self, RecordingStoreError>
    where
        P: AsRef<Path>,
    {
        let path: PathBuf = path.as_ref().to_path_buf();
        let manifest = RecordingStore::read_manifest(&path).await?;
        Ok(RecordingStore {
            path,
            manifest,
            current_entry: None,
        })
    }

    // Creates a new RecordingStore at the given path. This involves creating a dir
    // and writing an empty manifest.
    pub async fn create<P>(path: P) -> Result<Self, RecordingStoreError>
    where
        P: AsRef<Path>,
    {
        fs::create_dir_all(&path)
            .await
            .map_err(RecordingStoreError::OpenDirError)?;

        let mut store = RecordingStore {
            path: path.as_ref().to_owned(),
            manifest: Manifest {
                entries: Vec::new(),
            },
            current_entry: None,
        };

        store.write_manifest().await?;
        Ok(store)
    }

    // Does a best-effort attempt to recover the manifest from a directory of
    // QMDL files. We expect these files to be named like "<timestamp>.qmdl"
    // or "<timestamp>.qmdl.gz", and skip any files which don't match that
    // pattern.
    /// Open the store at `path`: load its manifest, rebuild the manifest from
    /// the recordings on disk when it is missing or unreadable, or create a
    /// fresh store when there is nothing there yet.
    pub async fn open_or_recover<P: AsRef<Path>>(path: P) -> Result<Self, RecordingStoreError> {
        let path = path.as_ref();
        let dir_exists = tokio::fs::try_exists(path)
            .await
            .map_err(RecordingStoreError::OpenDirError)?;
        let manifest_exists = dir_exists
            && tokio::fs::try_exists(path.join("manifest.toml"))
                .await
                .map_err(RecordingStoreError::ReadManifestError)?;
        if manifest_exists {
            match Self::load(path).await {
                Ok(store) => Ok(store),
                Err(RecordingStoreError::ParseManifestError(err)) => {
                    error!("failed to parse QMDL manifest: {err}");
                    info!("recovering manifest from existing QMDL files...");
                    Self::recover(path).await
                }
                Err(err) => Err(err),
            }
        } else if dir_exists {
            // The directory is there but the manifest is not. Reconstruct it
            // from the QMDL files on disk rather than starting fresh, which
            // would leave existing recordings physically present but
            // invisible to Rayhunter.
            warn!(
                "recording directory {} exists but manifest.toml is missing; recovering from QMDL files",
                path.display()
            );
            Self::recover(path).await
        } else {
            Self::create(path).await
        }
    }

    pub async fn recover<P>(path: P) -> Result<Self, RecordingStoreError>
    where
        P: AsRef<Path>,
    {
        let mut dir_entries = fs::read_dir(path.as_ref())
            .await
            .map_err(RecordingStoreError::OpenDirError)?;
        let mut manifest_entries = Vec::new();

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(RecordingStoreError::OpenDirError)?
        {
            let os_filename = entry.file_name();
            let Some(filename) = os_filename.to_str() else {
                continue;
            };

            let (stem, compressed) = if filename.ends_with(".qmdl") {
                (filename.trim_end_matches(".qmdl"), false)
            } else if filename.ends_with(".qmdl.gz") {
                (filename.trim_end_matches(".qmdl.gz"), true)
            } else {
                continue;
            };

            // Names are "<unix-seconds>" (legacy) or "<unix-seconds>-<sequence>"
            // (current). The start time is the seconds part in both, before any
            // hyphen.
            let seconds_part = stem.split('-').next().unwrap_or(stem);
            let Ok(start_timestamp) = seconds_part.parse::<i64>() else {
                warn!("QMDL file has invalid name {os_filename:?}, skipping");
                continue;
            };

            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(err) => {
                    warn!("failed to read QMDL file metadata: {err:?}, skipping");
                    continue;
                }
            };

            let Some(start_time) = DateTime::from_timestamp(start_timestamp, 0) else {
                warn!("QMDL filename {os_filename:?} gave an invalid timestamp, skipping");
                continue;
            };

            let Ok(last_message_time) = metadata.modified() else {
                warn!("failed to get modified time for QMDL file {os_filename:?}, skipping");
                continue;
            };

            info!("successfully recovered QMDL entry {os_filename:?}!");
            manifest_entries.push(ManifestEntry {
                name: stem.to_string(),
                compressed,
                start_time: start_time.into(),
                last_message_time: Some(last_message_time.into()),
                qmdl_size_bytes: metadata.size() as usize,
                rayhunter_version: None,
                system_os: None,
                arch: None,
                stop_reason: None,
                upload_time: None,
                gps_mode: None,
                // A recovered entry has no manifest to take these from.
                display_name: None,
                notes: None,
                telemetry_submission: None,
                telemetry_excluded: false,
            });
        }

        // sort chronologically
        manifest_entries.sort_by_key(|a| a.start_time);

        let mut store = RecordingStore {
            path: path.as_ref().to_path_buf(),
            manifest: Manifest {
                entries: manifest_entries,
            },
            current_entry: None,
        };
        store.write_manifest().await?;

        Ok(store)
    }

    async fn read_manifest<P>(path: P) -> Result<Manifest, RecordingStoreError>
    where
        P: AsRef<Path>,
    {
        let manifest_path = path.as_ref().join("manifest.toml");
        let file_contents = fs::read_to_string(&manifest_path)
            .await
            .map_err(RecordingStoreError::ReadManifestError)?;
        toml::from_str(&file_contents).map_err(RecordingStoreError::ParseManifestError)
    }

    // Closes the current entry (if needed), creates a new entry based on the
    // current time, and updates the manifest. Returns a tuple of the entry's
    // newly created QMDL file and analysis file.
    pub async fn new_entry(
        &mut self,
        gps_mode: GpsMode,
    ) -> Result<(File, File), RecordingStoreError> {
        // if we've already got an entry open, close it
        if self.current_entry.is_some() {
            self.close_current_entry().await?;
        }
        // Create the recording files exclusively: a name collision must return
        // an error, never truncate an existing recording. The name carries a
        // per-process sequence, so on the one path where a collision is possible
        // at all — a restart within the same second reusing a sequence value —
        // the retry gets a fresh name and makes progress.
        const MAX_ATTEMPTS: usize = 1024;
        let mut last_collision = None;
        for _ in 0..MAX_ATTEMPTS {
            let new_entry = ManifestEntry::new(gps_mode);
            let qmdl_filepath = new_entry.get_filepath(FileKind::Qmdl, &self.path);
            let qmdl_file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&qmdl_filepath)
                .await
            {
                Ok(file) => file,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    warn!(
                        "recording name {} already exists on disk, retrying with a new one",
                        new_entry.name
                    );
                    last_collision = Some(e);
                    continue;
                }
                Err(e) => return Err(RecordingStoreError::CreateFileError(e)),
            };
            // The QMDL name is now claimed. The companion files share its stem,
            // so they cannot pre-exist unless orphaned; create them exclusively
            // too and surface an orphan loudly rather than clobber it.
            let analysis_filepath = new_entry.get_filepath(FileKind::Analysis, &self.path);
            let analysis_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&analysis_filepath)
                .await
                .map_err(RecordingStoreError::CreateFileError)?;
            let gps_filepath = new_entry.get_filepath(FileKind::Gps, &self.path);
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&gps_filepath)
                .await
                .map_err(RecordingStoreError::CreateFileError)?;
            self.manifest.entries.push(new_entry);
            self.current_entry = Some(self.manifest.entries.len() - 1);
            self.write_manifest().await?;
            return Ok((qmdl_file, analysis_file));
        }
        Err(RecordingStoreError::CreateFileError(
            last_collision.unwrap_or_else(|| {
                io::Error::new(
                    ErrorKind::AlreadyExists,
                    "exhausted recording name attempts",
                )
            }),
        ))
    }

    pub async fn open_file(
        &self,
        entry_index: usize,
        file_kind: FileKind,
    ) -> Result<Option<File>, RecordingStoreError> {
        let entry = &self.manifest.entries[entry_index];
        let filepath = file_kind.get_filepath(&entry.name, &self.path, entry.compressed);

        match File::open(&filepath).await {
            Ok(file) => Ok(Some(file)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(RecordingStoreError::ReadFileError(e)),
        }
    }

    pub async fn open_entry_gps_for_append(
        &self,
        entry_index: usize,
    ) -> Result<Option<File>, RecordingStoreError> {
        let entry = &self.manifest.entries[entry_index];
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(entry.get_filepath(FileKind::Gps, &self.path))
            .await
        {
            Ok(file) => Ok(Some(file)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(RecordingStoreError::CreateFileError(e)),
        }
    }

    pub async fn clear_and_open_entry_analysis(
        &mut self,
        entry_index: usize,
    ) -> Result<File, RecordingStoreError> {
        let entry = &self.manifest.entries[entry_index];
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(entry.get_filepath(FileKind::Analysis, &self.path))
            .await
            .map_err(RecordingStoreError::ReadFileError)?;
        Ok(file)
    }

    // Unsets the current entry
    pub async fn close_current_entry(&mut self) -> Result<(), RecordingStoreError> {
        match self.current_entry {
            Some(entry_index) => {
                self.current_entry = None;
                // Add the closing clock readings to the recording's device
                // details. Not having them is no reason to fail the close.
                let entry = &self.manifest.entries[entry_index];
                let meta_path =
                    FileKind::Meta.get_filepath(&entry.name, &self.path, entry.compressed);
                if let Err(e) = crate::recording_metadata::finish(&meta_path).await {
                    warn!(
                        "couldn't finish recording metadata {}: {e}",
                        meta_path.display()
                    );
                }
                Ok(())
            }
            None => Err(RecordingStoreError::NoCurrentEntry),
        }
    }

    // Sets the current entry's size and updates the last_message_time to now, updating the manifest
    pub async fn update_current_entry_qmdl_size(
        &mut self,
        size_bytes: usize,
    ) -> Result<(), RecordingStoreError> {
        let Some(entry_index) = self.current_entry else {
            return Err(RecordingStoreError::NoCurrentEntry);
        };
        self.manifest.entries[entry_index].qmdl_size_bytes = size_bytes;
        self.manifest.entries[entry_index].last_message_time =
            Some(rayhunter::clock::get_adjusted_now());
        self.write_manifest().await
    }

    async fn write_manifest(&mut self) -> Result<(), RecordingStoreError> {
        // we don't technically need a mutable reference to `self` here, but it
        // does prevent multiple concurrent writes across different threads
        let tmp_path = self.path.join("manifest.toml.new");
        let mut manifest_tmp_file = File::create(&tmp_path)
            .await
            .map_err(RecordingStoreError::WriteManifestError)?;

        let manifest_contents =
            toml::to_string_pretty(&self.manifest).expect("failed to serialize manifest");
        manifest_tmp_file
            .write_all(manifest_contents.as_bytes())
            .await
            .map_err(RecordingStoreError::WriteManifestError)?;

        fs::rename(tmp_path, self.path.join("manifest.toml"))
            .await
            .map_err(RecordingStoreError::WriteManifestError)?;

        Ok(())
    }

    pub fn get_next_unuploaded_entry(&self, min_age: TimeDelta) -> Option<String> {
        let now = rayhunter::clock::get_adjusted_now();
        self.manifest
            .entries
            .iter()
            .filter_map(|entry| {
                if self.is_current_entry(&entry.name) || entry.upload_time.is_some() {
                    return None;
                }
                let age = now - entry.last_message_time.unwrap_or(entry.start_time);

                (age > min_age).then_some((&entry.name, age))
            })
            .max_by_key(|(_, age)| *age)
            .map(|(name, _)| name.clone())
    }

    // Finds an entry by filename
    pub fn entry_for_name(&self, name: &str) -> Option<(usize, &ManifestEntry)> {
        let entry_index = self
            .manifest
            .entries
            .iter()
            .position(|entry| entry.name == name)?;
        Some((entry_index, &self.manifest.entries[entry_index]))
    }

    pub fn get_current_entry(&self) -> Option<(usize, &ManifestEntry)> {
        let entry_index = self.current_entry?;
        Some((entry_index, &self.manifest.entries[entry_index]))
    }

    pub async fn set_current_stop_reason(
        &mut self,
        reason: String,
    ) -> Result<(), RecordingStoreError> {
        if let Some(idx) = self.current_entry {
            self.manifest.entries[idx].stop_reason = Some(reason);
            self.write_manifest().await?;
        }
        Ok(())
    }

    /// Set or clear the display name and notes for one recording.
    ///
    /// Both are optional and independent; `None` clears. Applies to any entry
    /// by name, including the one being recorded, so a recording can be
    /// labelled while it is still running.
    pub async fn set_entry_annotations(
        &mut self,
        name: &str,
        display_name: Option<String>,
        notes: Option<String>,
    ) -> Result<(), RecordingStoreError> {
        let idx = self
            .manifest
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .ok_or(RecordingStoreError::NoSuchEntryError)?;
        self.manifest.entries[idx].display_name = display_name;
        self.manifest.entries[idx].notes = notes;
        self.write_manifest().await?;
        Ok(())
    }

    pub async fn mark_entry_as_uploaded(
        &mut self,
        name: &str,
        upload_time: DateTime<Local>,
    ) -> Result<(), RecordingStoreError> {
        let entry_index = self
            .manifest
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .ok_or(RecordingStoreError::NoSuchEntryError)?;
        self.manifest.entries[entry_index].upload_time = Some(upload_time);
        self.write_manifest().await?;
        Ok(())
    }

    /// Note that a recording was contributed.
    pub async fn mark_entry_submitted(
        &mut self,
        name: &str,
        submission: TelemetrySubmission,
    ) -> Result<(), RecordingStoreError> {
        let (entry_index, _) = self
            .entry_for_name(name)
            .ok_or(RecordingStoreError::NoSuchEntryError)?;
        self.manifest.entries[entry_index].telemetry_submission = Some(submission);
        self.write_manifest().await?;
        Ok(())
    }

    /// Note that a contribution was withdrawn. The record stays, marked, so
    /// the recording is not contributed again.
    pub async fn mark_entry_withdrawn(
        &mut self,
        name: &str,
        withdrawn_at: DateTime<Local>,
    ) -> Result<(), RecordingStoreError> {
        let (entry_index, _) = self
            .entry_for_name(name)
            .ok_or(RecordingStoreError::NoSuchEntryError)?;
        match &mut self.manifest.entries[entry_index].telemetry_submission {
            Some(submission) => submission.withdrawn_at = Some(withdrawn_at),
            None => return Err(RecordingStoreError::NoSuchEntryError),
        }
        self.write_manifest().await?;
        Ok(())
    }

    /// Keep a recording out of, or let it back into, the contribution queue.
    pub async fn set_entry_telemetry_excluded(
        &mut self,
        name: &str,
        excluded: bool,
    ) -> Result<(), RecordingStoreError> {
        let (entry_index, _) = self
            .entry_for_name(name)
            .ok_or(RecordingStoreError::NoSuchEntryError)?;
        self.manifest.entries[entry_index].telemetry_excluded = excluded;
        self.write_manifest().await?;
        Ok(())
    }

    pub fn is_current_entry(&self, name: &str) -> bool {
        match self.current_entry {
            Some(idx) => match self.manifest.entries.get(idx) {
                Some(entry) => entry.name == name,
                None => false,
            },
            None => false,
        }
    }

    pub async fn delete_entry(&mut self, name: &str) -> Result<(), RecordingStoreError> {
        let entry_to_delete_idx = self
            .manifest
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .ok_or(RecordingStoreError::NoSuchEntryError)?;
        // Delete the files *first*, and only drop the manifest entry once they
        // are gone. Doing it the other way round means a failed file deletion
        // leaves the recording invisible to Rayhunter but still on disk — the
        // bulk-delete path already avoids that, and this matches it.
        let entry_to_delete = &self.manifest.entries[entry_to_delete_idx];
        for &file_kind in FileKind::ALL {
            let filepath = file_kind.get_filepath(
                &entry_to_delete.name,
                &self.path,
                entry_to_delete.compressed,
            );
            remove_file_if_exists(&filepath)
                .await
                .map_err(RecordingStoreError::DeleteFileError)?;
        }

        match self.current_entry {
            Some(current_entry) if current_entry == entry_to_delete_idx => {
                self.close_current_entry().await?;
            }
            Some(current_entry) if current_entry > entry_to_delete_idx => {
                self.current_entry = Some(current_entry - 1);
            }
            _ => {}
        };
        self.manifest.entries.remove(entry_to_delete_idx);
        self.write_manifest().await?;
        Ok(())
    }

    pub async fn delete_all_entries(&mut self) -> Result<(), RecordingStoreError> {
        if self.current_entry.is_some() {
            self.close_current_entry().await?;
        }

        let mut keep = Vec::new();

        'entries: for entry in &self.manifest.entries {
            for &file_kind in FileKind::ALL {
                let filepath = file_kind.get_filepath(&entry.name, &self.path, entry.compressed);
                if let Err(e) = remove_file_if_exists(&filepath).await {
                    log::warn!("failed to remove {filepath:?}: {e:?}");
                    // Some error happened with deleting this entry, abort and go to the next one.
                    // Also *keep* the manifest entry.
                    keep.push(true);
                    continue 'entries;
                }
            }

            keep.push(false);
        }

        let mut keep_iter = keep.into_iter();
        self.manifest.entries.retain(|_| keep_iter.next().unwrap());
        self.write_manifest().await?;
        Ok(())
    }
}

async fn remove_file_if_exists(path: &Path) -> Result<(), io::Error> {
    match tokio::fs::remove_file(path).await {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        res => res,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{Builder, TempDir};

    fn make_temp_dir() -> TempDir {
        Builder::new().prefix("qmdl_store_test").tempdir().unwrap()
    }

    #[tokio::test]
    async fn test_load_from_empty_dir() {
        let dir = make_temp_dir();
        let manifest = dir.path().join("manifest.toml");
        assert!(!fs::try_exists(&manifest).await.unwrap());
        let _created_store = RecordingStore::create(dir.path()).await.unwrap();
        assert!(fs::try_exists(&manifest).await.unwrap());
        let loaded_store = RecordingStore::load(dir.path()).await.unwrap();
        assert_eq!(loaded_store.manifest.entries.len(), 0);
    }

    #[tokio::test]
    async fn test_creating_updating_and_closing_entries() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let entry_index = store.current_entry.unwrap();
        assert_eq!(
            RecordingStore::read_manifest(dir.path()).await.unwrap(),
            store.manifest
        );
        assert!(
            store.manifest.entries[entry_index]
                .last_message_time
                .is_none()
        );

        store.update_current_entry_qmdl_size(1000).await.unwrap();
        let (entry_index, entry) = store
            .entry_for_name(&store.manifest.entries[entry_index].name)
            .unwrap();
        assert!(entry.last_message_time.is_some());
        assert_eq!(store.manifest.entries[entry_index].qmdl_size_bytes, 1000);
        assert_eq!(
            RecordingStore::read_manifest(dir.path()).await.unwrap(),
            store.manifest
        );

        store.close_current_entry().await.unwrap();
        assert!(matches!(
            store.close_current_entry().await,
            Err(RecordingStoreError::NoCurrentEntry)
        ));
    }

    #[tokio::test]
    async fn test_create_on_existing_store() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        store.update_current_entry_qmdl_size(1000).await.unwrap();
        let store = RecordingStore::create(dir.path()).await.unwrap();
        assert_eq!(store.manifest.entries.len(), 0);
    }

    #[tokio::test]
    async fn test_repeated_new_entries() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let entry_index = store.current_entry.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let new_entry_index = store.current_entry.unwrap();
        assert_ne!(entry_index, new_entry_index);
        assert_eq!(store.manifest.entries.len(), 2);
    }

    /// Two recordings created in quick succession (the same clock second) must
    /// get different names, and starting the second must never truncate the
    /// first's QMDL file. This is the collision/overwrite hazard.
    #[tokio::test]
    async fn two_entries_in_the_same_second_do_not_collide_or_truncate() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();

        let (mut qmdl_a, _) = store.new_entry(GpsMode::Disabled).await.unwrap();
        let name_a = store.manifest.entries[0].name.clone();
        qmdl_a
            .write_all(b"evidence from recording A")
            .await
            .unwrap();
        qmdl_a.flush().await.unwrap();
        let path_a = FileKind::Qmdl.get_filepath(&name_a, dir.path(), true);

        // Start a second recording immediately; in practice this is the same
        // wall-clock second.
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let name_b = store.manifest.entries[1].name.clone();

        assert_ne!(name_a, name_b, "names must differ within a second");
        let a_after = fs::read(&path_a).await.unwrap();
        assert_eq!(
            a_after, b"evidence from recording A",
            "recording A's QMDL must not be truncated by starting B"
        );
    }

    /// Creating an entry whose QMDL file already exists on disk must not
    /// silently truncate it. Exclusive creation retries onto a fresh name.
    #[tokio::test]
    async fn an_existing_qmdl_file_is_never_truncated() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        // Pre-place a file at the exact name the next entry would try first.
        let now = rayhunter::clock::get_adjusted_now().timestamp();
        let seq = ENTRY_SEQUENCE.load(Ordering::Relaxed);
        let squatted = format!("{now}-{seq}");
        let squatted_path = FileKind::Qmdl.get_filepath(&squatted, dir.path(), true);
        fs::write(&squatted_path, b"do not clobber me")
            .await
            .unwrap();

        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let new_name = store.manifest.entries[0].name.clone();

        // The squatted file is untouched...
        assert_eq!(
            fs::read(&squatted_path).await.unwrap(),
            b"do not clobber me"
        );
        // ...and the new recording took a different name if it collided.
        if new_name == squatted {
            panic!("new entry reused the squatted name, which means it truncated it");
        }
    }

    /// A directory of QMDL files with no manifest must be recoverable, and the
    /// old bare-seconds names must still parse alongside the new ones.
    #[tokio::test]
    async fn recovers_entries_when_the_manifest_is_missing() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let new_name = store.manifest.entries[0].name.clone();
        store.close_current_entry().await.unwrap();
        // A legacy bare-seconds recording alongside it.
        fs::write(
            FileKind::Qmdl.get_filepath("1725123456", dir.path(), true),
            b"legacy",
        )
        .await
        .unwrap();

        // Lose the manifest, then recover.
        fs::remove_file(dir.path().join("manifest.toml"))
            .await
            .unwrap();
        let recovered = RecordingStore::recover(dir.path()).await.unwrap();

        let names: Vec<&str> = recovered
            .manifest
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            names.contains(&new_name.as_str()),
            "new-format name recovered"
        );
        assert!(names.contains(&"1725123456"), "legacy name recovered");
    }

    /// Deleting a recording whose files cannot be removed must leave the entry
    /// in the manifest, so it stays discoverable rather than becoming an
    /// orphan. Here the QMDL is made undeletable by removing it out from under
    /// the store and replacing the path with a non-empty directory, which
    /// `remove_file` refuses.
    #[tokio::test]
    async fn a_failed_delete_keeps_the_entry_discoverable() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let name = store.manifest.entries[0].name.clone();
        store.close_current_entry().await.unwrap();

        // Replace the QMDL file path with a non-empty directory so that
        // remove_file fails on it.
        let qmdl_path = FileKind::Qmdl.get_filepath(&name, dir.path(), true);
        fs::remove_file(&qmdl_path).await.unwrap();
        fs::create_dir(&qmdl_path).await.unwrap();
        fs::write(qmdl_path.join("blocker"), b"x").await.unwrap();

        let result = store.delete_entry(&name).await;
        assert!(result.is_err(), "delete should report the failure");
        assert!(
            store.entry_for_name(&name).is_some(),
            "the recording must remain in the manifest after a failed delete"
        );
    }

    #[tokio::test]
    async fn test_delete_all_entries() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        assert!(store.current_entry.is_some());

        store.delete_all_entries().await.unwrap();
        assert!(store.current_entry.is_none());

        // regression test: deleting all entries should also work when there's no current
        // recording.
        store.delete_all_entries().await.unwrap();
        assert!(store.current_entry.is_none());
    }

    #[tokio::test]
    async fn test_mark_entry_as_uploaded_sets_time_and_persists() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        let name = store.manifest.entries[0].name.clone();
        store.close_current_entry().await.unwrap();

        let upload_time = Local::now();
        store
            .mark_entry_as_uploaded(&name, upload_time)
            .await
            .unwrap();
        assert_eq!(store.manifest.entries[0].upload_time, Some(upload_time));

        let reloaded = RecordingStore::load(dir.path()).await.unwrap();
        assert_eq!(reloaded.manifest.entries[0].upload_time, Some(upload_time));
    }

    #[tokio::test]
    async fn test_mark_entry_as_uploaded_missing_entry() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();
        assert!(matches!(
            store.mark_entry_as_uploaded("nope", Local::now()).await,
            Err(RecordingStoreError::NoSuchEntryError)
        ));
    }

    #[tokio::test]
    async fn test_get_next_unuploaded_entry() {
        let dir = make_temp_dir();
        let mut store = RecordingStore::create(dir.path()).await.unwrap();

        for _ in 0..3 {
            let _ = store.new_entry(GpsMode::Disabled).await.unwrap();
        }

        store.manifest.entries[0].name = "entry-0".to_owned();
        store.manifest.entries[0].start_time = Local::now() - TimeDelta::seconds(10);
        store.manifest.entries[0].last_message_time = None;

        store.manifest.entries[1].name = "entry-1".to_owned();
        store.manifest.entries[1].start_time = Local::now() - TimeDelta::seconds(10);
        store.manifest.entries[1].last_message_time = Some(Local::now() - TimeDelta::seconds(5));

        store.manifest.entries[2].name = "entry-2".to_owned();
        store.manifest.entries[2].start_time = Local::now() - TimeDelta::seconds(10);
        store.manifest.entries[2].last_message_time = Some(Local::now() - TimeDelta::seconds(1));

        assert_eq!(
            store.get_next_unuploaded_entry(TimeDelta::seconds(3600)),
            None,
        );

        assert_eq!(
            store.get_next_unuploaded_entry(TimeDelta::seconds(3)),
            Some("entry-0".to_owned())
        );
        store
            .mark_entry_as_uploaded("entry-0", Local::now())
            .await
            .unwrap();
        assert_eq!(
            store.get_next_unuploaded_entry(TimeDelta::seconds(3)),
            Some("entry-1".to_owned())
        );
        store
            .mark_entry_as_uploaded("entry-1", Local::now())
            .await
            .unwrap();
        assert_eq!(store.get_next_unuploaded_entry(TimeDelta::seconds(3)), None);
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::{MAX_DISPLAY_NAME, sanitize_display_name};

    #[test]
    fn ordinary_names_survive_intact() {
        assert_eq!(sanitize_display_name("cafe-visit"), "cafe-visit");
        assert_eq!(sanitize_display_name("Protest_2026"), "Protest_2026");
        assert_eq!(sanitize_display_name("walk3"), "walk3");
    }

    /// The name becomes the name of a downloaded file, so it must not be able
    /// to steer a path or escape the directory it lands in.
    #[test]
    fn path_separators_and_traversal_cannot_survive() {
        assert!(!sanitize_display_name("../../etc/passwd").contains('/'));
        assert!(!sanitize_display_name("..\\..\\windows").contains('\\'));
        assert!(!sanitize_display_name("../../etc/passwd").contains(".."));
        assert_eq!(sanitize_display_name("/"), "");
        assert_eq!(sanitize_display_name("."), "");
        assert_eq!(sanitize_display_name(".."), "");
    }

    /// It also lands in a quoted HTTP header, so a quote getting through would
    /// let a name break out of the filename and add header content of its own.
    #[test]
    fn quotes_and_control_characters_cannot_survive() {
        let hostile = "a\"; filename=\"evil.sh";
        let cleaned = sanitize_display_name(hostile);
        assert!(!cleaned.contains('"'));
        assert!(!cleaned.contains(';'));
        assert!(!sanitize_display_name("a\r\nSet-Cookie: x=y").contains('\n'));
        assert!(!sanitize_display_name("a\r\nSet-Cookie: x=y").contains('\r'));
        assert!(!sanitize_display_name("null\0byte").contains('\0'));
    }

    #[test]
    fn a_name_is_capped_at_the_documented_length() {
        let long = "x".repeat(200);
        assert_eq!(
            sanitize_display_name(&long).chars().count(),
            MAX_DISPLAY_NAME
        );
    }

    /// A name that reduces to nothing must come back empty rather than as a
    /// row of underscores, so the caller can reject it and say why.
    #[test]
    fn a_name_of_only_punctuation_reduces_to_nothing() {
        assert_eq!(sanitize_display_name("!!!"), "");
        assert_eq!(sanitize_display_name("   "), "");
        assert_eq!(sanitize_display_name(""), "");
        assert_eq!(sanitize_display_name("日本語"), "");
    }

    #[test]
    fn inner_punctuation_becomes_underscores_rather_than_vanishing() {
        assert_eq!(sanitize_display_name("cafe visit"), "cafe_visit");
        assert_eq!(sanitize_display_name("a.b.c"), "a_b_c");
    }
}
