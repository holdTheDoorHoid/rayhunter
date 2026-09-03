//! What happens to a submission once every part has arrived: the summary
//! part is opened with the ingest key, `telemetry.json` is read out of it,
//! and the record is updated. The capture part is never opened here.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use async_zip::tokio::read::seek::ZipFileReader;
use futures::AsyncReadExt;
use telemetry_format::keys::RecipientPrivateKey;
use telemetry_format::manifest::{Manifest, PartKind};
use telemetry_format::stream::{info_for, open};
use telemetry_format::summary::Summary;
use tokio::fs;

use crate::store;

/// Decrypt one part to a file, on the blocking pool.
async fn open_part(
    key: &RecipientPrivateKey,
    submission_id: &str,
    part_name: &str,
    sealed: &Path,
    plain: &Path,
) -> anyhow::Result<u64> {
    let key = key.clone();
    let info = info_for(submission_id, part_name);
    let sealed = sealed.to_path_buf();
    let plain = plain.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let input = std::fs::File::open(&sealed)?;
        let output = std::fs::File::create(&plain)?;
        let result = open(
            &key,
            &info,
            std::io::BufReader::new(input),
            std::io::BufWriter::new(output),
        );
        match result {
            Ok(opened) => Ok(opened.plaintext_bytes),
            Err(e) => {
                let _ = std::fs::remove_file(&plain);
                Err(anyhow!("{e}"))
            }
        }
    })
    .await?
}

/// One named file out of a zip, or `None` when it is not there.
pub async fn extract_entry(zip_path: &Path, name: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let file = fs::File::open(zip_path).await?;
    let mut zip = ZipFileReader::with_tokio(tokio::io::BufReader::new(file)).await?;
    let index = zip
        .file()
        .entries()
        .iter()
        .position(|entry| entry.filename().as_str().ok() == Some(name));
    let Some(index) = index else {
        return Ok(None);
    };
    let mut reader = zip.reader_with_entry(index).await?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    Ok(Some(bytes))
}

/// The names inside a zip.
pub async fn list_entries(zip_path: &Path) -> anyhow::Result<Vec<String>> {
    let file = fs::File::open(zip_path).await?;
    let zip = ZipFileReader::with_tokio(tokio::io::BufReader::new(file)).await?;
    Ok(zip
        .file()
        .entries()
        .iter()
        .filter_map(|entry| entry.filename().as_str().ok().map(String::from))
        .collect())
}

/// Open the summary and read what it says. Called once every part is in.
pub async fn finalize(
    data: &Path,
    ingest_key: &RecipientPrivateKey,
    id: &str,
    manifest: &Manifest,
) -> anyhow::Result<Summary> {
    let dir = store::dir_for(data, id).ok_or_else(|| anyhow!("bad id"))?;
    let part = manifest
        .parts
        .iter()
        .find(|p| p.kind == PartKind::Summary)
        .ok_or_else(|| anyhow!("no summary part"))?;
    let sealed = dir.join("parts").join(&part.name);
    let summary_zip = dir.join("summary.zip");
    let bytes = open_part(ingest_key, id, &part.name, &sealed, &summary_zip)
        .await
        .context("opening the summary part")?;
    if bytes != part.plaintext_bytes {
        let _ = fs::remove_file(&summary_zip).await;
        return Err(anyhow!(
            "the summary opened to {bytes} bytes, the manifest said {}",
            part.plaintext_bytes
        ));
    }
    let telemetry = extract_entry(&summary_zip, "telemetry.json")
        .await
        .context("reading the summary bundle")?
        .ok_or_else(|| anyhow!("the summary bundle has no telemetry.json"))?;
    let summary: Summary = serde_json::from_slice(&telemetry).context("parsing telemetry.json")?;
    if summary.submission_id != id {
        return Err(anyhow!(
            "telemetry.json names submission {}, not {id}",
            summary.submission_id
        ));
    }
    if summary.format != telemetry_format::FORMAT {
        return Err(anyhow!("telemetry.json is format {:?}", summary.format));
    }
    fs::write(dir.join("summary.json"), &telemetry).await?;
    Ok(summary)
}

/// Decrypt a full submission's capture with the archive key.
pub async fn decrypt_capture(
    data: &Path,
    id: &str,
    archive_key: &RecipientPrivateKey,
    out: &Path,
) -> anyhow::Result<()> {
    let manifest = store::parsed_manifest(data, id)
        .await?
        .ok_or_else(|| anyhow!("no submission {id}"))?;
    let part = manifest
        .parts
        .iter()
        .find(|p| p.kind == PartKind::Capture)
        .ok_or_else(|| anyhow!("{id} has no capture part; it is a summary submission"))?;
    if archive_key.public_key().key_id() != part.recipient_key_id {
        return Err(anyhow!(
            "the capture was encrypted to key {}, not to this one ({})",
            part.recipient_key_id,
            archive_key.public_key().key_id()
        ));
    }
    let dir = store::dir_for(data, id).ok_or_else(|| anyhow!("bad id"))?;
    let sealed: PathBuf = dir.join("parts").join(&part.name);
    let bytes = open_part(archive_key, id, &part.name, &sealed, out).await?;
    if bytes != part.plaintext_bytes {
        let _ = fs::remove_file(out).await;
        return Err(anyhow!(
            "the capture opened to {bytes} bytes, the manifest said {}",
            part.plaintext_bytes
        ));
    }
    Ok(())
}
