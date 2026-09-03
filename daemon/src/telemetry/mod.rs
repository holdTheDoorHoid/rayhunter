//! Contributing recordings to a community-run dataset.
//!
//! Off unless the owner turns it on. What it does, tier by tier, and why
//! the collection side is shaped as it is, are in `telemetry/DESIGN.md`.
//! This module holds the shared state, the status the settings page shows,
//! and the API the page uses; the work is in [`worker`], the decisions in
//! [`bundle`], the wire in [`client`], the keys in [`keystore`].

pub mod bundle;
pub mod client;
pub mod keystore;
pub mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use chrono::Local;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use telemetry_format::keys::RecipientPublicKey;
use telemetry_format::manifest::{ServerInfo, Tier, WithdrawRequest};
use telemetry_format::summary::LocationPrecision;
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock};

use crate::config::TelemetryConfig;
use crate::server::ServerState;
use client::Collector;
use keystore::KeyStore;

/// A recording the worker is not sending, and why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

/// What the settings page shows about contributions.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct TelemetryStatus {
    pub enabled: bool,
    pub server_url: String,
    pub server_name: Option<String>,
    pub tier: Option<Tier>,
    pub location: Option<LocationPrecision>,
    pub submitter_key_id: Option<String>,
    pub key_created_at: Option<String>,
    /// Whether the background worker is running this session.
    pub worker_running: bool,
    /// "ready", "nothing to send", or why uploads are waiting.
    pub network: String,
    /// Recordings that will be sent when the network allows.
    pub queued: Vec<String>,
    /// Recordings that will not be sent, with the reason.
    pub skipped: Vec<Skipped>,
    /// The recording being built or uploaded right now.
    pub busy: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<String>,
    pub submitted_count: u32,
    /// The service presented keys other than the pinned ones. Nothing is
    /// sent until the owner checks the server again.
    pub server_keys_changed: bool,
}

/// Shared between the worker and the API.
pub struct TelemetryState {
    pub config: TelemetryConfig,
    pub auth_store_path: PathBuf,
    pub status: RwLock<TelemetryStatus>,
    /// Wakes the worker for a round now.
    pub kick: Notify,
    keys: Mutex<Option<KeyStore>>,
}

impl TelemetryState {
    pub fn new(config: TelemetryConfig, auth_store_path: PathBuf) -> Self {
        let status = TelemetryStatus {
            enabled: config.enabled,
            server_url: config.server_url.clone(),
            server_name: config.server_name.clone(),
            tier: Some(config.tier),
            location: Some(config.location),
            network: if config.enabled {
                "not checked yet".to_string()
            } else {
                "off".to_string()
            },
            ..Default::default()
        };
        TelemetryState {
            config,
            auth_store_path,
            status: RwLock::new(status),
            kick: Notify::new(),
            keys: Mutex::new(None),
        }
    }

    /// The signing keys, opened on first use so that a unit which never
    /// contributes never makes one.
    pub async fn keys(&self) -> MutexGuard<'_, Option<KeyStore>> {
        let mut guard = self.keys.lock().await;
        if guard.is_none() {
            match KeyStore::open(&self.auth_store_path, self.config.key_rotation_days).await {
                Ok(store) => {
                    let mut status = self.status.write().await;
                    status.submitter_key_id = Some(store.key_id().to_string());
                    status.key_created_at = Some(store.created_at().to_rfc3339());
                    *guard = Some(store);
                }
                Err(e) => warn!("could not open the contribution signing keys: {e}"),
            }
        }
        guard
    }
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/telemetry/status",
    tag = "Community dataset",
    responses((status = StatusCode::OK, description = "What the contribution worker is doing", body = TelemetryStatus)),
    summary = "Contribution status",
    description = "Whether contributing is on, which recordings are queued or skipped and why, when the last upload happened, and whether the service's keys still match the pinned ones."
))]
pub async fn get_status(State(state): State<Arc<ServerState>>) -> Json<TelemetryStatus> {
    Json(state.telemetry.status.read().await.clone())
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ProbeRequest {
    pub url: String,
}

/// What a service says about itself, with its keys named the way a person
/// can compare them.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ProbeResponse {
    pub info: ServerInfo,
    pub ingest_key_id: String,
    pub ingest_fingerprint: String,
    pub archive_key_id: Option<String>,
    pub archive_fingerprint: Option<String>,
    /// Whether the keys match what this unit has pinned. Absent when
    /// nothing is pinned yet.
    pub matches_pinned: Option<bool>,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/telemetry/probe",
    tag = "Community dataset",
    request_body = ProbeRequest,
    responses(
        (status = StatusCode::OK, description = "The service's description", body = ProbeResponse),
        (status = StatusCode::BAD_REQUEST, description = "The URL is not acceptable, or the service did not answer as one")
    ),
    summary = "Check a collection service",
    description = "Fetch a service's description and key fingerprints so the owner can decide whether to pin them. Changes nothing."
))]
pub async fn probe_server(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<ProbeRequest>,
) -> Result<Json<ProbeResponse>, (StatusCode, String)> {
    TelemetryConfig::acceptable_server_url(&request.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let collector = Collector::new(&request.url, Duration::from_secs(30))
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let info = collector
        .info()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let ingest = RecipientPublicKey::from_base64(&info.ingest_public_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("the service's ingest key is unreadable: {e}"),
        )
    })?;
    let archive = match &info.archive_public_key {
        Some(key) => Some(RecipientPublicKey::from_base64(key).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("the service's archive key is unreadable: {e}"),
            )
        })?),
        None => None,
    };
    let pinned = &state.telemetry.config;
    let matches_pinned = pinned.ingest_public_key.as_deref().map(|p| {
        let ingest_matches = p.trim() == info.ingest_public_key.trim();
        let archive_matches = match (&pinned.archive_public_key, &info.archive_public_key) {
            (Some(a), Some(b)) => a.trim() == b.trim(),
            (None, _) => true,
            (Some(_), None) => false,
        };
        ingest_matches && archive_matches
    });
    Ok(Json(ProbeResponse {
        ingest_key_id: ingest.key_id(),
        ingest_fingerprint: ingest.fingerprint(),
        archive_key_id: archive.as_ref().map(|k| k.key_id()),
        archive_fingerprint: archive.as_ref().map(|k| k.fingerprint()),
        matches_pinned,
        info,
    }))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/telemetry/send-now",
    tag = "Community dataset",
    responses(
        (status = StatusCode::ACCEPTED, description = "The worker will run a round now"),
        (status = StatusCode::CONFLICT, description = "Contributing is off")
    ),
    summary = "Contribute now",
    description = "Ask the worker to look for eligible recordings without waiting for its next poll. The network policy still applies."
))]
pub async fn send_now(
    State(state): State<Arc<ServerState>>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !state.telemetry.status.read().await.worker_running {
        return Err((
            StatusCode::CONFLICT,
            "contributing is off, or the worker is not running".to_string(),
        ));
    }
    state.telemetry.kick.notify_one();
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct KeyRotated {
    pub key_id: String,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/telemetry/rotate-key",
    tag = "Community dataset",
    responses(
        (status = StatusCode::OK, description = "A new signing key is current", body = KeyRotated),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "The key store could not be written")
    ),
    summary = "New signing identity",
    description = "Retire the unit's current signing key and make a new one, so later contributions cannot be linked to earlier ones. Earlier ones can still be withdrawn."
))]
pub async fn rotate_key(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<KeyRotated>, (StatusCode, String)> {
    let mut keys = state.telemetry.keys().await;
    let store = keys.as_mut().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "the key store is unavailable".to_string(),
    ))?;
    store
        .rotate()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let key_id = store.key_id().to_string();
    let mut status = state.telemetry.status.write().await;
    status.submitter_key_id = Some(key_id.clone());
    status.key_created_at = Some(store.created_at().to_rfc3339());
    Ok(Json(KeyRotated { key_id }))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/telemetry/withdraw/{name}",
    tag = "Community dataset",
    params(("name" = String, Path, description = "The recording whose contribution to withdraw")),
    responses(
        (status = StatusCode::OK, description = "The service was told to remove it, and it will not be sent again"),
        (status = StatusCode::NOT_FOUND, description = "No such recording, or it was never contributed"),
        (status = StatusCode::CONFLICT, description = "The key that signed it has been forgotten"),
        (status = StatusCode::BAD_GATEWAY, description = "The service refused or did not answer")
    ),
    summary = "Withdraw a contribution",
    description = "Send the service a signed request to delete this recording's submission. Works only with the key that signed it, which is why retired keys are kept."
))]
pub async fn withdraw_submission(
    State(state): State<Arc<ServerState>>,
    AxumPath(name): AxumPath<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let submission = {
        let store = state.qmdl_store_lock.read().await;
        store
            .entry_for_name(&name)
            .and_then(|(_, e)| e.telemetry_submission.clone())
            .ok_or((
                StatusCode::NOT_FOUND,
                format!("recording {name} was not contributed"),
            ))?
    };
    if submission.withdrawn_at.is_some() {
        return Ok(StatusCode::OK);
    }
    let (body, signature) = {
        let keys = state.telemetry.keys().await;
        let store = keys.as_ref().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "the key store is unavailable".to_string(),
        ))?;
        let key = store.key_for(&submission.key_id).await.ok_or((
            StatusCode::CONFLICT,
            format!(
                "the key that signed this contribution ({}) is no longer on this unit",
                submission.key_id
            ),
        ))?;
        let request = WithdrawRequest {
            format: telemetry_format::FORMAT.to_string(),
            submission_id: submission.submission_id.clone(),
            requested_at: rayhunter::clock::get_adjusted_now().to_rfc3339(),
            reason: None,
        };
        let body = serde_json::to_vec(&request)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let signature = key.sign(&body);
        (body, signature)
    };
    let collector = Collector::new(&submission.server_url, Duration::from_secs(60))
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    collector
        .withdraw(&submission.submission_id, body, &signature)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    state
        .qmdl_store_lock
        .write()
        .await
        .mark_entry_withdrawn(&name, Local::now())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?;
    info!(
        "withdrew contribution {} of recording {name}",
        submission.submission_id
    );
    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ExcludeRequest {
    pub excluded: bool,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/telemetry/exclude/{name}",
    tag = "Community dataset",
    params(("name" = String, Path, description = "The recording")),
    request_body = ExcludeRequest,
    responses(
        (status = StatusCode::OK, description = "Recorded"),
        (status = StatusCode::NOT_FOUND, description = "No such recording")
    ),
    summary = "Keep a recording out of contributions",
    description = "Mark a recording as never to be contributed, or let it back in. Does not withdraw one already sent; use the withdraw call for that."
))]
pub async fn set_excluded(
    State(state): State<Arc<ServerState>>,
    AxumPath(name): AxumPath<String>,
    Json(request): Json<ExcludeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .qmdl_store_lock
        .write()
        .await
        .set_entry_telemetry_excluded(&name, request.excluded)
        .await
        .map_err(|e| match e {
            crate::qmdl_store::RecordingStoreError::NoSuchEntryError => {
                (StatusCode::NOT_FOUND, format!("no recording named {name}"))
            }
            other => (StatusCode::INTERNAL_SERVER_ERROR, format!("{other:?}")),
        })?;
    Ok(StatusCode::OK)
}
