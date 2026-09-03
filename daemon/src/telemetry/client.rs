//! Talking to the collection service.
//!
//! Four requests: read the service's description, open a submission with a
//! signed manifest, put each encrypted part, and finalize. A fifth withdraws.
//! Every signed body is sent exactly as signed, with the signature in a
//! header, so the service verifies the bytes it received rather than a
//! re-serialisation of them.

use std::path::Path;
use std::time::Duration;

use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use reqwest::{Body, Client, StatusCode};
use serde::Deserialize;
use telemetry_format::manifest::{SIGNATURE_HEADER, ServerInfo, WELL_KNOWN_PATH};
use tokio_util::io::ReaderStream;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{0}")]
    Request(#[from] reqwest::Error),
    #[error("the service answered {status}: {body}")]
    Refused { status: StatusCode, body: String },
    #[error("the service's description is not readable: {0}")]
    BadInfo(String),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

/// What the service answers when a submission is opened.
#[derive(Debug, Clone, Deserialize)]
pub struct Opened {
    pub submission_id: String,
}

pub struct Collector {
    client: Client,
    base: String,
}

impl Collector {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, ClientError> {
        let client = crate::http_client::builder().timeout(timeout).build()?;
        Ok(Collector {
            client,
            base: base_url.trim().trim_end_matches('/').to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// The service's description: name, keys, what it accepts.
    pub async fn info(&self) -> Result<ServerInfo, ClientError> {
        let response = self
            .client
            .get(self.url(WELL_KNOWN_PATH))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(refused(status, &body));
        }
        if body.len() > 64 * 1024 {
            return Err(ClientError::BadInfo("description is too large".into()));
        }
        let info: ServerInfo =
            serde_json::from_slice(&body).map_err(|e| ClientError::BadInfo(e.to_string()))?;
        if info.format != telemetry_format::FORMAT {
            return Err(ClientError::BadInfo(format!(
                "it speaks {:?}, this unit speaks {:?}",
                info.format,
                telemetry_format::FORMAT
            )));
        }
        Ok(info)
    }

    /// Open a submission. `manifest` is the exact bytes that were signed.
    pub async fn open(&self, manifest: Vec<u8>, signature: &str) -> Result<Opened, ClientError> {
        let response = self
            .client
            .post(self.url("/v1/submissions"))
            .header(CONTENT_TYPE, "application/json")
            .header(SIGNATURE_HEADER, signature)
            .body(manifest)
            .send()
            .await?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(refused(status, &body));
        }
        serde_json::from_slice(&body).map_err(|e| ClientError::BadInfo(e.to_string()))
    }

    /// Upload one encrypted part from a file, streaming it.
    pub async fn put_part(
        &self,
        submission_id: &str,
        part_name: &str,
        path: &Path,
    ) -> Result<(), ClientError> {
        let file = tokio::fs::File::open(path).await?;
        let len = file.metadata().await?.len();
        let response = self
            .client
            .put(self.url(&format!(
                "/v1/submissions/{submission_id}/parts/{part_name}"
            )))
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, len)
            .body(Body::wrap_stream(ReaderStream::new(file)))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.bytes().await.unwrap_or_default();
            return Err(refused(status, &body));
        }
        Ok(())
    }

    /// Tell the service every part is there. `signature` is over
    /// [`telemetry_format::manifest::finalize_message`].
    pub async fn finalize(&self, submission_id: &str, signature: &str) -> Result<(), ClientError> {
        let response = self
            .client
            .post(self.url(&format!("/v1/submissions/{submission_id}/finalize")))
            .header(SIGNATURE_HEADER, signature)
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.bytes().await.unwrap_or_default();
            return Err(refused(status, &body));
        }
        Ok(())
    }

    /// Ask for a submission to be removed. `body` is the exact signed bytes
    /// of a [`telemetry_format::manifest::WithdrawRequest`].
    pub async fn withdraw(
        &self,
        submission_id: &str,
        body: Vec<u8>,
        signature: &str,
    ) -> Result<(), ClientError> {
        let response = self
            .client
            .post(self.url(&format!("/v1/submissions/{submission_id}/withdraw")))
            .header(CONTENT_TYPE, "application/json")
            .header(SIGNATURE_HEADER, signature)
            .body(body)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            let body = response.bytes().await.unwrap_or_default();
            return Err(refused(status, &body));
        }
        Ok(())
    }
}

fn refused(status: StatusCode, body: &[u8]) -> ClientError {
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    let body = if text.is_empty() {
        status.canonical_reason().unwrap_or("no detail").to_string()
    } else {
        text.chars().take(200).collect()
    };
    ClientError::Refused { status, body }
}
