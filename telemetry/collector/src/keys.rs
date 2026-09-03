//! The service's key files.
//!
//! `ingest.key` is what the server needs to open summary parts as they
//! arrive. `archive.key` opens raw captures and must live somewhere else;
//! the server only ever sees `archive.pub`. Both private files are written
//! mode 0600 and hold the base64 secret and nothing else.

use std::path::Path;

use anyhow::{Context, anyhow};
use telemetry_format::keys::{RecipientPrivateKey, RecipientPublicKey};
use tokio::fs;

pub struct ServingKeys {
    pub ingest_private: RecipientPrivateKey,
    pub ingest_public: RecipientPublicKey,
    pub archive_public: Option<RecipientPublicKey>,
}

pub async fn keygen(out: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(out).await?;
    for name in ["ingest", "archive"] {
        let key_path = out.join(format!("{name}.key"));
        if fs::try_exists(&key_path).await? {
            return Err(anyhow!(
                "{} exists already; refusing to overwrite a key",
                key_path.display()
            ));
        }
    }
    for name in ["ingest", "archive"] {
        let (sk, pk) = RecipientPrivateKey::generate();
        write_private(&out.join(format!("{name}.key")), &sk.to_base64()).await?;
        fs::write(out.join(format!("{name}.pub")), pk.to_base64() + "\n").await?;
        println!(
            "{name}: key id {}  fingerprint {}",
            pk.key_id(),
            pk.fingerprint()
        );
    }
    println!(
        "Move {} off this machine before serving. The server needs ingest.key, ingest.pub and archive.pub only.",
        out.join("archive.key").display()
    );
    Ok(())
}

async fn write_private(path: &Path, secret: &str) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, format!("{secret}\n")).await?;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

pub async fn load_private(path: &Path) -> anyhow::Result<RecipientPrivateKey> {
    let text = fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    RecipientPrivateKey::from_base64(text.trim()).map_err(|e| anyhow!("{}: {e}", path.display()))
}

pub async fn load_public(path: &Path) -> anyhow::Result<RecipientPublicKey> {
    let text = fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    RecipientPublicKey::from_base64(text.trim()).map_err(|e| anyhow!("{}: {e}", path.display()))
}

pub async fn load_for_serving(dir: &Path) -> anyhow::Result<ServingKeys> {
    if fs::try_exists(dir.join("archive.key")).await? {
        return Err(anyhow!(
            "{} is on this machine; the server must not hold the archive key. Move it away and keep archive.pub only.",
            dir.join("archive.key").display()
        ));
    }
    let ingest_private = load_private(&dir.join("ingest.key")).await?;
    let ingest_public = load_public(&dir.join("ingest.pub")).await?;
    if ingest_private.public_key() != ingest_public {
        return Err(anyhow!("ingest.key and ingest.pub do not match"));
    }
    let archive_path = dir.join("archive.pub");
    let archive_public = if fs::try_exists(&archive_path).await? {
        Some(load_public(&archive_path).await?)
    } else {
        None
    };
    Ok(ServingKeys {
        ingest_private,
        ingest_public,
        archive_public,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn keygen_writes_four_files_and_serving_refuses_the_archive_key() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("keys");
        keygen(&out).await.unwrap();
        for name in ["ingest.key", "ingest.pub", "archive.key", "archive.pub"] {
            assert!(out.join(name).exists(), "{name}");
        }
        assert!(keygen(&out).await.is_err(), "never overwrite a key");
        assert!(
            load_for_serving(&out).await.is_err(),
            "the archive key is still here"
        );
        std::fs::remove_file(out.join("archive.key")).unwrap();
        let keys = load_for_serving(&out).await.unwrap();
        assert!(keys.archive_public.is_some());
        assert_eq!(keys.ingest_private.public_key(), keys.ingest_public);
    }
}
