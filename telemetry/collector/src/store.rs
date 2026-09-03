//! Where submissions live on disk, and what is known about each.
//!
//! One directory per submission under `<data>/submissions/`, holding the
//! manifest exactly as received, its signature, the encrypted parts, the
//! decrypted summary once finalized, and `state.json`. Files rather than a
//! database: the whole store can be read with `ls` and backed up with `cp`,
//! and nothing here needs a query a directory listing cannot answer.

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use telemetry_format::manifest::{Manifest, Tier};
use telemetry_format::summary::Summary;
use tokio::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Opened; parts still arriving.
    Pending,
    /// Every part arrived, verified and the summary opened. Awaiting review.
    Received,
    /// A person looked and it may be published.
    Verified,
    /// A person looked and it may not.
    Rejected,
    /// The unit asked for it to be removed. Payloads deleted.
    Withdrawn,
}

impl Status {
    pub fn parse(text: &str) -> Option<Status> {
        Some(match text {
            "pending" => Status::Pending,
            "received" => Status::Received,
            "verified" => Status::Verified,
            "rejected" => Status::Rejected,
            "withdrawn" => Status::Withdrawn,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Received => "received",
            Status::Verified => "verified",
            Status::Rejected => "rejected",
            Status::Withdrawn => "withdrawn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    pub reviewed_at: String,
}

/// `state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub submission_id: String,
    pub received_at: String,
    pub status: Status,
    pub tier: Tier,
    pub submitter_key_id: String,
    #[serde(default)]
    pub parts_received: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// The worst severity in the summary, copied out at finalize so a
    /// listing needs no other file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_severity: Option<String>,
    #[serde(default)]
    pub warning_count: u32,
}

impl Record {
    pub fn one_line(&self) -> String {
        let tags = self
            .review
            .as_ref()
            .map(|r| r.tags.join(","))
            .unwrap_or_default();
        format!(
            "{}  {:<9}  {:<7}  {}  {:<6}  {:>3} warnings  {}",
            self.submission_id,
            self.status.as_str(),
            self.tier,
            &self.received_at[..self.received_at.len().min(19)],
            self.max_severity.as_deref().unwrap_or("-"),
            self.warning_count,
            tags
        )
    }
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn submissions_dir(data: &Path) -> PathBuf {
    data.join("submissions")
}

pub fn dir_for(data: &Path, id: &str) -> Option<PathBuf> {
    telemetry_format::is_submission_id(id).then(|| submissions_dir(data).join(id))
}

pub async fn save(data: &Path, record: &Record) -> anyhow::Result<()> {
    let dir = dir_for(data, &record.submission_id).ok_or_else(|| anyhow!("bad id"))?;
    fs::create_dir_all(&dir).await?;
    let tmp = dir.join("state.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(record)?).await?;
    fs::rename(&tmp, dir.join("state.json")).await?;
    Ok(())
}

pub async fn load(data: &Path, id: &str) -> anyhow::Result<Option<Record>> {
    let Some(dir) = dir_for(data, id) else {
        return Ok(None);
    };
    match fs::read(dir.join("state.json")).await {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn load_manifest(data: &Path, id: &str) -> anyhow::Result<Option<(Vec<u8>, String)>> {
    let Some(dir) = dir_for(data, id) else {
        return Ok(None);
    };
    let bytes = match fs::read(dir.join("manifest.json")).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let signature = fs::read_to_string(dir.join("manifest.sig")).await?;
    Ok(Some((bytes, signature.trim().to_string())))
}

pub async fn parsed_manifest(data: &Path, id: &str) -> anyhow::Result<Option<Manifest>> {
    Ok(match load_manifest(data, id).await? {
        Some((bytes, _)) => Some(serde_json::from_slice(&bytes)?),
        None => None,
    })
}

pub async fn load_summary(data: &Path, id: &str) -> anyhow::Result<Option<Summary>> {
    let Some(dir) = dir_for(data, id) else {
        return Ok(None);
    };
    match fs::read(dir.join("summary.json")).await {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Every record, newest first.
pub async fn list(data: &Path) -> anyhow::Result<Vec<Record>> {
    let mut out = Vec::new();
    let dir = submissions_dir(data);
    let mut entries = match fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(id) = name.to_str() else { continue };
        if let Some(record) = load(data, id).await? {
            out.push(record);
        }
    }
    out.sort_by(|a, b| b.received_at.cmp(&a.received_at));
    Ok(out)
}

pub async fn review(
    data: &Path,
    id: &str,
    status: Status,
    tags: Vec<String>,
    note: Option<String>,
    reviewer: Option<String>,
) -> anyhow::Result<()> {
    let mut record = load(data, id)
        .await?
        .ok_or_else(|| anyhow!("no submission {id}"))?;
    match record.status {
        Status::Received | Status::Verified | Status::Rejected => {}
        Status::Pending => return Err(anyhow!("{id} has not finished arriving")),
        Status::Withdrawn => return Err(anyhow!("{id} was withdrawn by the unit")),
    }
    record.status = status;
    record.review = Some(Review {
        tags,
        note,
        reviewer,
        reviewed_at: now(),
    });
    save(data, &record).await
}

/// Remove everything the unit sent, keeping the record as a tombstone so
/// the same id cannot be reused.
pub async fn withdraw(data: &Path, id: &str) -> anyhow::Result<bool> {
    let Some(mut record) = load(data, id).await? else {
        return Ok(false);
    };
    let dir = dir_for(data, id).ok_or_else(|| anyhow!("bad id"))?;
    let _ = fs::remove_dir_all(dir.join("parts")).await;
    for file in ["summary.zip", "summary.json"] {
        let _ = fs::remove_file(dir.join(file)).await;
    }
    record.status = Status::Withdrawn;
    record.withdrawn_at = Some(now());
    record.review = None;
    save(data, &record).await?;
    Ok(true)
}

/// Bytes under the data directory, for the disk cap.
pub async fn disk_usage(data: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![data.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, status: Status) -> Record {
        Record {
            submission_id: id.into(),
            received_at: now(),
            status,
            tier: Tier::Summary,
            submitter_key_id: "abcd".into(),
            parts_received: vec![],
            finalized_at: None,
            review: None,
            withdrawn_at: None,
            failure: None,
            max_severity: None,
            warning_count: 0,
        }
    }

    #[tokio::test]
    async fn records_round_trip_and_ids_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        save(dir.path(), &record(id, Status::Received))
            .await
            .unwrap();
        let back = load(dir.path(), id).await.unwrap().unwrap();
        assert_eq!(back.status, Status::Received);
        assert!(load(dir.path(), "../etc").await.unwrap().is_none());
        assert!(dir_for(dir.path(), "../etc").is_none());
        assert_eq!(list(dir.path()).await.unwrap().len(), 1);

        review(
            dir.path(),
            id,
            Status::Verified,
            vec!["interesting".into()],
            Some("worth a look".into()),
            None,
        )
        .await
        .unwrap();
        let back = load(dir.path(), id).await.unwrap().unwrap();
        assert_eq!(back.status, Status::Verified);
        assert_eq!(back.review.as_ref().unwrap().tags, vec!["interesting"]);

        assert!(withdraw(dir.path(), id).await.unwrap());
        let back = load(dir.path(), id).await.unwrap().unwrap();
        assert_eq!(back.status, Status::Withdrawn);
        assert!(back.review.is_none());
        assert!(
            review(dir.path(), id, Status::Verified, vec![], None, None)
                .await
                .is_err()
        );
        assert!(
            !withdraw(dir.path(), "ffffffffffffffffffffffffffffffff")
                .await
                .unwrap()
        );
    }
}
