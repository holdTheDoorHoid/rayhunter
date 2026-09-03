//! The unit's signing keys, kept beside its TLS key.
//!
//! A submission is signed so the service can refuse forgeries and honour a
//! withdrawal from the same unit. The key is rotated on a schedule so the
//! service can group one unit's submissions for a while (it needs that for
//! rate limiting) but cannot follow the unit for years. Retired keys are
//! kept, because a submission made under one can only be withdrawn by it.
//!
//! Layout under `<auth_store_path>/telemetry/`, all mode 0600 in a 0700
//! directory: `current.pem` with `current.json` beside it saying when it was
//! made, and `retired/<key_id>.pem` for each earlier key.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use telemetry_format::keys::SubmitterKey;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyRecord {
    key_id: String,
    created_at: DateTime<Local>,
}

/// The current key and its bookkeeping.
pub struct KeyStore {
    dir: PathBuf,
    current: SubmitterKey,
    record: KeyRecord,
}

impl KeyStore {
    /// Open the store, making a key if there is none, and replacing one that
    /// is older than `rotation_days` (zero never rotates).
    pub async fn open(auth_store_path: &Path, rotation_days: u32) -> std::io::Result<Self> {
        let dir = auth_store_path.join("telemetry");
        fs::create_dir_all(&dir).await?;
        set_mode(&dir, 0o700).await?;
        fs::create_dir_all(dir.join("retired")).await?;
        set_mode(&dir.join("retired"), 0o700).await?;

        let loaded = load_current(&dir).await;
        let mut store = match loaded {
            Some((key, record)) => KeyStore {
                dir,
                current: key,
                record,
            },
            None => {
                let store = KeyStore::fresh(dir).await?;
                info!(
                    "made a new contribution signing key {}",
                    store.record.key_id
                );
                store
            }
        };
        if rotation_days > 0 {
            let age = Local::now() - store.record.created_at;
            if age > Duration::days(rotation_days as i64) {
                info!(
                    "the contribution signing key is {} days old, rotating it",
                    age.num_days()
                );
                store.rotate().await?;
            }
        }
        Ok(store)
    }

    async fn fresh(dir: PathBuf) -> std::io::Result<Self> {
        let key = SubmitterKey::generate();
        let record = KeyRecord {
            key_id: key.key_id(),
            created_at: Local::now(),
        };
        let store = KeyStore {
            dir,
            current: key,
            record,
        };
        store.write_current().await?;
        Ok(store)
    }

    async fn write_current(&self) -> std::io::Result<()> {
        let pem = self
            .current
            .to_pkcs8_pem()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        write_private(&self.dir.join("current.pem"), pem.as_bytes()).await?;
        let json = serde_json::to_vec_pretty(&self.record)?;
        write_private(&self.dir.join("current.json"), &json).await
    }

    /// Retire the current key and make a new one.
    pub async fn rotate(&mut self) -> std::io::Result<()> {
        let retired_path = self
            .dir
            .join("retired")
            .join(format!("{}.pem", self.record.key_id));
        let pem = self
            .current
            .to_pkcs8_pem()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        write_private(&retired_path, pem.as_bytes()).await?;
        let fresh = KeyStore::fresh(self.dir.clone()).await?;
        info!(
            "contribution signing key {} retired, {} is current",
            self.record.key_id, fresh.record.key_id
        );
        self.current = fresh.current;
        self.record = fresh.record;
        Ok(())
    }

    pub fn current(&self) -> &SubmitterKey {
        &self.current
    }

    pub fn key_id(&self) -> &str {
        &self.record.key_id
    }

    pub fn created_at(&self) -> DateTime<Local> {
        self.record.created_at
    }

    /// The key with this id: the current one, or a retired one still on
    /// disk. `None` when it has been forgotten.
    pub async fn key_for(&self, key_id: &str) -> Option<SubmitterKey> {
        if key_id == self.record.key_id {
            return Some(self.current.clone());
        }
        if !key_id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return None;
        }
        let path = self.dir.join("retired").join(format!("{key_id}.pem"));
        let pem = fs::read_to_string(&path).await.ok()?;
        match SubmitterKey::from_pkcs8_pem(&pem) {
            Ok(key) if key.key_id() == key_id => Some(key),
            Ok(_) => {
                warn!("retired key file {} holds a different key", path.display());
                None
            }
            Err(e) => {
                warn!("retired key file {} is unreadable: {e}", path.display());
                None
            }
        }
    }
}

async fn load_current(dir: &Path) -> Option<(SubmitterKey, KeyRecord)> {
    let pem = fs::read_to_string(dir.join("current.pem")).await.ok()?;
    let key = match SubmitterKey::from_pkcs8_pem(&pem) {
        Ok(key) => key,
        Err(e) => {
            warn!("the contribution signing key is unreadable, making a new one: {e}");
            return None;
        }
    };
    let record: KeyRecord = match fs::read(dir.join("current.json")).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(KeyRecord {
            key_id: key.key_id(),
            created_at: Local::now(),
        }),
        Err(_) => KeyRecord {
            key_id: key.key_id(),
            created_at: Local::now(),
        },
    };
    // The record's id is derived, never trusted over the key itself.
    let record = KeyRecord {
        key_id: key.key_id(),
        ..record
    };
    Some((key, record))
}

async fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).await?;
    set_mode(&tmp, 0o600).await?;
    fs::rename(&tmp, path).await
}

async fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn a_key_is_made_once_and_reloaded_after() {
        let dir = tempfile::tempdir().unwrap();
        let first = KeyStore::open(dir.path(), 30).await.unwrap();
        let id = first.key_id().to_string();
        drop(first);
        let again = KeyStore::open(dir.path(), 30).await.unwrap();
        assert_eq!(again.key_id(), id);
        assert_eq!(again.current().key_id(), id);

        let telemetry_dir = dir.path().join("telemetry");
        let mode = std::fs::metadata(&telemetry_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        let mode = std::fs::metadata(telemetry_dir.join("current.pem"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn rotation_keeps_the_old_key_for_withdrawals() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = KeyStore::open(dir.path(), 0).await.unwrap();
        let old_id = store.key_id().to_string();
        let old_public = store.current().public_key_base64();
        store.rotate().await.unwrap();
        assert_ne!(store.key_id(), old_id);
        let old = store
            .key_for(&old_id)
            .await
            .expect("retired key still there");
        assert_eq!(old.public_key_base64(), old_public);
        assert!(store.key_for("0000000000000000").await.is_none());
        assert!(store.key_for("../current").await.is_none());
    }

    #[tokio::test]
    async fn an_old_key_is_rotated_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::open(dir.path(), 30).await.unwrap();
        let old_id = store.key_id().to_string();
        // Age the record by rewriting it.
        let record = KeyRecord {
            key_id: old_id.clone(),
            created_at: Local::now() - Duration::days(31),
        };
        std::fs::write(
            dir.path().join("telemetry/current.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        drop(store);
        let store = KeyStore::open(dir.path(), 30).await.unwrap();
        assert_ne!(store.key_id(), old_id);
        assert!(store.key_for(&old_id).await.is_some());
        // With rotation off, the same aged key stays.
        let untouched = KeyStore::open(dir.path(), 0).await.unwrap();
        assert_eq!(untouched.key_id(), store.key_id());
    }
}
