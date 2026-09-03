//! The background job that contributes eligible recordings.
//!
//! It wakes on a timer or when asked, checks the network policy, confirms
//! the service still presents the pinned keys, then takes eligible
//! recordings oldest first: build, encrypt, sign, upload, record. One
//! failure stops the round and backs off; nothing is retried in a tight
//! loop on a device with one core that is also recording.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use chrono::{Local, TimeDelta};
use log::{info, warn};
use rayhunter::Device;
use telemetry_format::keys::RecipientPublicKey;
use telemetry_format::manifest::{
    ClientInfo, Consent, Manifest, PartInfo, PartKind, Tier, finalize_message,
};
use telemetry_format::stream::{info_for, seal};
use tokio::select;
use tokio::sync::RwLock;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use wifi_station::{WifiState, WifiStatus};

use super::bundle::{self, Plan, ReportFacts, Skip};
use super::client::Collector;
use super::{Skipped, TelemetryState};
use crate::config::{TelemetryConfig, TelemetryNetwork};
use crate::qmdl_store::{FileKind, RecordingStore, TelemetrySubmission};

/// Whether the network policy allows an upload right now.
pub fn network_ok(
    policy: TelemetryNetwork,
    allowed: &[String],
    wifi: &WifiStatus,
) -> Result<(), String> {
    match policy {
        TelemetryNetwork::Any => Ok(()),
        TelemetryNetwork::WifiOnly => {
            if wifi.state != WifiState::Connected {
                return Err("waiting for the WiFi client to join a network".to_string());
            }
            if allowed.is_empty() {
                return Ok(());
            }
            match &wifi.ssid {
                Some(ssid) if allowed.iter().any(|a| a == ssid) => Ok(()),
                Some(ssid) => Err(format!(
                    "joined to {ssid}, which is not one of the allowed networks"
                )),
                None => Err("joined to a network whose name is unknown".to_string()),
            }
        }
    }
}

/// How long to wait after `failures` consecutive failures: a quarter of an
/// hour, doubling, capped at six hours.
pub fn backoff(failures: u32) -> Duration {
    let quarter: u64 = 15 * 60;
    let secs = quarter.saturating_mul(1u64 << failures.min(5).saturating_sub(1).min(4));
    Duration::from_secs(secs.min(6 * 3600))
}

pub struct WorkerDeps {
    pub config: TelemetryConfig,
    pub device: Device,
    pub store: Arc<RwLock<RecordingStore>>,
    pub wifi_status: Arc<RwLock<WifiStatus>>,
    pub state: Arc<TelemetryState>,
}

pub fn run_telemetry_worker(
    task_tracker: &TaskTracker,
    shutdown: CancellationToken,
    deps: WorkerDeps,
) {
    task_tracker.spawn(async move {
        {
            let mut status = deps.state.status.write().await;
            status.worker_running = true;
        }
        let mut ticker = interval(Duration::from_secs(deps.config.poll_interval_secs.max(30)));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut failures: u32 = 0;
        let mut wait_until: Option<Instant> = None;
        loop {
            let kicked = select! {
                _ = shutdown.cancelled() => break,
                _ = ticker.tick() => false,
                _ = deps.state.kick.notified() => true,
            };
            if !kicked && let Some(until) = wait_until && Instant::now() < until {
                continue;
            }
            wait_until = None;
            match run_round(&deps).await {
                Ok(()) => failures = 0,
                Err(err) => {
                    failures += 1;
                    let wait = backoff(failures);
                    warn!("contribution round failed ({failures} in a row): {err:#}; next try in {} min", wait.as_secs() / 60);
                    wait_until = Some(Instant::now() + wait);
                    let mut status = deps.state.status.write().await;
                    status.last_error = Some(format!("{err:#}"));
                    status.next_attempt_at = Some((Local::now() + TimeDelta::from_std(wait).unwrap_or_default()).to_rfc3339());
                }
            }
        }
        deps.state.status.write().await.worker_running = false;
    });
}

/// One pass: refresh the queue, and contribute what is ready.
async fn run_round(deps: &WorkerDeps) -> anyhow::Result<()> {
    let config = &deps.config;
    let now = Local::now();
    {
        let mut status = deps.state.status.write().await;
        status.last_attempt_at = Some(now.to_rfc3339());
        status.next_attempt_at = None;
        status.last_error = None;
    }

    // What is waiting, by the manifest alone.
    let min_age = TimeDelta::try_seconds(config.min_age_secs).unwrap_or_default();
    let (mut candidates, mut skipped) = {
        let store = deps.store.read().await;
        let mut ready = Vec::new();
        let mut skipped = Vec::new();
        for entry in &store.manifest.entries {
            let is_current = store.is_current_entry(&entry.name);
            match bundle::check_manifest(entry, is_current, min_age, now) {
                Ok(()) => ready.push((entry.start_time, entry.name.clone())),
                Err(skip) => {
                    // The routine reasons are not worth listing.
                    if !matches!(skip, Skip::AlreadySent | Skip::Withdrawn | Skip::Current) {
                        skipped.push(Skipped {
                            name: entry.name.clone(),
                            reason: skip.reason().to_string(),
                        });
                    }
                }
            }
        }
        ready.sort();
        (
            ready.into_iter().map(|(_, n)| n).collect::<Vec<_>>(),
            skipped,
        )
    };

    // The report decides the rest, and is read once here for the queue and
    // again only for the recordings that are actually sent.
    let mut queue = Vec::new();
    for name in candidates.drain(..) {
        let Some(file) = open(&deps.store, &name, FileKind::Analysis).await? else {
            skipped.push(Skipped {
                name,
                reason: Skip::NotAnalysed.reason().to_string(),
            });
            continue;
        };
        let facts = bundle::read_report(file).await;
        match bundle::check_report(&facts, config) {
            Ok(()) => queue.push((name, facts)),
            Err(skip) => skipped.push(Skipped {
                name,
                reason: skip.reason().to_string(),
            }),
        }
    }
    {
        let mut status = deps.state.status.write().await;
        status.queued = queue.iter().map(|(n, _)| n.clone()).collect();
        status.skipped = skipped;
    }
    if queue.is_empty() {
        deps.state.status.write().await.network = "nothing to send".to_string();
        return Ok(());
    }

    // The network policy.
    {
        let wifi = deps.wifi_status.read().await;
        if let Err(reason) = network_ok(config.network, &config.allowed_networks, &wifi) {
            deps.state.status.write().await.network = reason;
            return Ok(());
        }
    }
    deps.state.status.write().await.network = "ready".to_string();

    // The service, and that its keys are the pinned ones.
    let collector = Collector::new(
        &config.server_url,
        Duration::from_secs(config.upload_timeout_secs),
    )?;
    let info = collector
        .info()
        .await
        .context("reading the service's description")?;
    let ingest = pinned_key(config.ingest_public_key.as_deref(), &info.ingest_public_key)
        .map_err(|e| anyhow!("ingest key: {e}"))?;
    let archive = match config.tier {
        Tier::Full => {
            let presented = info
                .archive_public_key
                .as_deref()
                .ok_or_else(|| anyhow!("the service no longer offers an archive key"))?;
            Some(
                pinned_key(config.archive_public_key.as_deref(), presented)
                    .map_err(|e| anyhow!("archive key: {e}"))?,
            )
        }
        Tier::Summary => None,
    };
    if !info.accepted_tiers.contains(&config.tier) {
        return Err(anyhow!(
            "the service does not accept {} submissions",
            config.tier
        ));
    }
    {
        let mut status = deps.state.status.write().await;
        status.server_keys_changed = false;
        status.server_name = Some(info.name.clone());
    }

    for (name, facts) in queue {
        deps.state.status.write().await.busy = Some(name.clone());
        let result = submit_one(
            deps,
            &collector,
            &name,
            &facts,
            &ingest,
            archive.as_ref(),
            &info,
        )
        .await;
        deps.state.status.write().await.busy = None;
        match result {
            Ok(id) => {
                info!("contributed recording {name} as submission {id}");
                let mut status = deps.state.status.write().await;
                status.last_success_at = Some(Local::now().to_rfc3339());
                status.submitted_count += 1;
                status.queued.retain(|n| n != &name);
            }
            Err(err) => return Err(err.context(format!("contributing recording {name}"))),
        }
    }
    Ok(())
}

/// The key the service presents, if it is the one pinned in the config.
fn pinned_key(pinned: Option<&str>, presented: &str) -> Result<RecipientPublicKey, String> {
    let presented = RecipientPublicKey::from_base64(presented)
        .map_err(|e| format!("the service's key is unreadable: {e}"))?;
    let pinned = RecipientPublicKey::from_base64(pinned.ok_or("no key is pinned")?)
        .map_err(|e| format!("the pinned key is unreadable: {e}"))?;
    if pinned != presented {
        return Err(format!(
            "the service now presents key {} but {} is pinned; check the server again in settings if you trust the change",
            presented.key_id(),
            pinned.key_id()
        ));
    }
    Ok(pinned)
}

async fn open(
    store: &Arc<RwLock<RecordingStore>>,
    name: &str,
    kind: FileKind,
) -> anyhow::Result<Option<tokio::fs::File>> {
    let store = store.read().await;
    let Some((index, _)) = store.entry_for_name(name) else {
        return Ok(None);
    };
    Ok(store.open_file(index, kind).await?)
}

/// Encrypt one built file to `recipient`, on the blocking pool since the
/// work is CPU and file bound.
async fn seal_file(
    recipient: &RecipientPublicKey,
    submission_id: &str,
    part_name: &str,
    plain: &Path,
    sealed: &Path,
) -> anyhow::Result<telemetry_format::stream::Sealed> {
    let recipient = recipient.clone();
    let info = info_for(submission_id, part_name);
    let plain = plain.to_path_buf();
    let sealed = sealed.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let input = std::fs::File::open(&plain)?;
        let output = std::fs::File::create(&sealed)?;
        let summary = seal(
            &recipient,
            &info,
            std::io::BufReader::new(input),
            std::io::BufWriter::new(output),
        )?;
        // The plaintext is not needed once sealed, and it is the sensitive
        // one of the two.
        let _ = std::fs::remove_file(&plain);
        Ok::<_, anyhow::Error>(summary)
    })
    .await?
}

async fn submit_one(
    deps: &WorkerDeps,
    collector: &Collector,
    name: &str,
    facts: &ReportFacts,
    ingest: &RecipientPublicKey,
    archive: Option<&RecipientPublicKey>,
    info: &telemetry_format::manifest::ServerInfo,
) -> anyhow::Result<String> {
    let config = &deps.config;
    let entry = {
        let store = deps.store.read().await;
        store
            .entry_for_name(name)
            .map(|(_, e)| e.clone())
            .ok_or_else(|| anyhow!("recording {name} is gone"))?
    };

    // Room to build: the summary is at most the capture's size again, the
    // full tier twice that, plus the sealed copies.
    let spool_root = {
        let store = deps.store.read().await;
        store.path.join("telemetry-spool")
    };
    let needed = (entry.qmdl_size_bytes as u64)
        .saturating_mul(if config.tier == Tier::Full { 4 } else { 2 })
        .saturating_add(8 * 1024 * 1024);
    if let Ok(disk) =
        crate::stats::DiskStats::new(&spool_root.parent().unwrap_or(&spool_root).to_string_lossy())
        && let Some(available) = disk.available_bytes
        && available < needed
    {
        return Err(anyhow!(
            "not enough free space to build the bundle ({} MB free, {} MB needed)",
            available / (1024 * 1024),
            needed / (1024 * 1024)
        ));
    }

    let submission_id = telemetry_format::new_submission_id();
    let spool = spool_root.join(&submission_id);
    tokio::fs::create_dir_all(&spool).await?;
    let result = submit_in_spool(
        deps,
        collector,
        &entry,
        facts,
        ingest,
        archive,
        info,
        &submission_id,
        &spool,
    )
    .await;
    // Whatever happened, nothing sensitive stays in the spool.
    if let Err(e) = tokio::fs::remove_dir_all(&spool).await {
        warn!("could not clean up {}: {e}", spool.display());
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn submit_in_spool(
    deps: &WorkerDeps,
    collector: &Collector,
    entry: &crate::qmdl_store::ManifestEntry,
    facts: &ReportFacts,
    ingest: &RecipientPublicKey,
    archive: Option<&RecipientPublicKey>,
    info: &telemetry_format::manifest::ServerInfo,
    submission_id: &str,
    spool: &Path,
) -> anyhow::Result<String> {
    let config = &deps.config;
    let consent = Consent {
        tier: config.tier,
        acknowledged_at: match config.tier {
            Tier::Full => config.full_tier_acknowledged_at.clone(),
            Tier::Summary => None,
        },
    };
    let plan = Plan {
        config,
        device: &deps.device,
        submission_id: submission_id.to_string(),
        consent: consent.clone(),
    };
    let built = bundle::build(&deps.store, entry, facts, &plan, spool)
        .await
        .context("building the bundle")?;
    info!(
        "built {} bundle for recording {}: {} warnings, {} cells, location {}",
        config.tier,
        entry.name,
        built.summary.analysis.warnings.total(),
        built.summary.cells.len(),
        built
            .summary
            .location
            .as_ref()
            .map(|l| format!("{:?}", l.precision))
            .unwrap_or_else(|| "none".to_string())
    );

    // Seal.
    let mut parts = Vec::new();
    let summary_enc = spool.join("summary.enc");
    let sealed = seal_file(
        ingest,
        submission_id,
        "summary.enc",
        &built.summary_zip,
        &summary_enc,
    )
    .await
    .context("encrypting the summary")?;
    if sealed.ciphertext_bytes > info.max_summary_bytes {
        return Err(anyhow!(
            "the summary is {} MB, more than the service accepts ({} MB)",
            sealed.ciphertext_bytes / (1024 * 1024),
            info.max_summary_bytes / (1024 * 1024)
        ));
    }
    parts.push(PartInfo {
        name: "summary.enc".into(),
        kind: PartKind::Summary,
        recipient_key_id: ingest.key_id(),
        plaintext_bytes: sealed.plaintext_bytes,
        ciphertext_bytes: sealed.ciphertext_bytes,
        sha256: sealed.sha256,
    });
    let capture_enc = spool.join("capture.enc");
    if let (Some(capture_zip), Some(archive)) = (&built.capture_zip, archive) {
        let sealed = seal_file(
            archive,
            submission_id,
            "capture.enc",
            capture_zip,
            &capture_enc,
        )
        .await
        .context("encrypting the capture")?;
        if sealed.ciphertext_bytes > info.max_capture_bytes {
            return Err(anyhow!(
                "the capture is {} MB, more than the service accepts ({} MB)",
                sealed.ciphertext_bytes / (1024 * 1024),
                info.max_capture_bytes / (1024 * 1024)
            ));
        }
        parts.push(PartInfo {
            name: "capture.enc".into(),
            kind: PartKind::Capture,
            recipient_key_id: archive.key_id(),
            plaintext_bytes: sealed.plaintext_bytes,
            ciphertext_bytes: sealed.ciphertext_bytes,
            sha256: sealed.sha256,
        });
    }

    // Sign and send.
    let (manifest_bytes, signature, finalize_signature, key_id) = {
        let mut keys = deps.state.keys().await;
        let keys = keys
            .as_mut()
            .ok_or_else(|| anyhow!("the signing key store is unavailable"))?;
        let manifest = Manifest {
            format: telemetry_format::FORMAT.to_string(),
            submission_id: submission_id.to_string(),
            created_at: rayhunter::clock::get_adjusted_now().to_rfc3339(),
            tier: config.tier,
            submitter_public_key: keys.current().public_key_base64(),
            consent,
            client: ClientInfo {
                name: "rayhunter".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            parts: parts.clone(),
        };
        let bytes = serde_json::to_vec(&manifest)?;
        let signature = keys.current().sign(&bytes);
        let finalize = keys.current().sign(&finalize_message(submission_id));
        (bytes, signature, finalize, keys.key_id().to_string())
    };
    let opened = collector
        .open(manifest_bytes, &signature)
        .await
        .context("opening the submission")?;
    if opened.submission_id != submission_id {
        return Err(anyhow!(
            "the service answered with a different submission id"
        ));
    }
    for part in &parts {
        let path = spool.join(&part.name);
        collector
            .put_part(submission_id, &part.name, &path)
            .await
            .with_context(|| format!("uploading {}", part.name))?;
    }
    collector
        .finalize(submission_id, &finalize_signature)
        .await
        .context("finalizing the submission")?;

    deps.store
        .write()
        .await
        .mark_entry_submitted(
            &entry.name,
            TelemetrySubmission {
                submission_id: submission_id.to_string(),
                tier: config.tier,
                submitted_at: Local::now(),
                key_id,
                server_url: config.server_url.clone(),
                withdrawn_at: None,
            },
        )
        .await
        .map_err(|e| anyhow!("recording the submission: {e:?}"))?;
    Ok(submission_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wifi(state: WifiState, ssid: Option<&str>) -> WifiStatus {
        WifiStatus {
            state,
            ssid: ssid.map(String::from),
            ip: None,
            error: None,
            tx_packets: None,
        }
    }

    #[test]
    fn wifi_only_waits_for_a_connection_and_respects_the_allow_list() {
        let none: Vec<String> = Vec::new();
        assert!(
            network_ok(
                TelemetryNetwork::WifiOnly,
                &none,
                &wifi(WifiState::Disabled, None)
            )
            .is_err()
        );
        assert!(
            network_ok(
                TelemetryNetwork::WifiOnly,
                &none,
                &wifi(WifiState::Connecting, None)
            )
            .is_err()
        );
        assert!(
            network_ok(
                TelemetryNetwork::WifiOnly,
                &none,
                &wifi(WifiState::Connected, Some("home"))
            )
            .is_ok()
        );
        let allowed = vec!["home".to_string()];
        assert!(
            network_ok(
                TelemetryNetwork::WifiOnly,
                &allowed,
                &wifi(WifiState::Connected, Some("home"))
            )
            .is_ok()
        );
        assert!(
            network_ok(
                TelemetryNetwork::WifiOnly,
                &allowed,
                &wifi(WifiState::Connected, Some("cafe"))
            )
            .is_err()
        );
        assert!(
            network_ok(
                TelemetryNetwork::WifiOnly,
                &allowed,
                &wifi(WifiState::Connected, None)
            )
            .is_err()
        );
        // "Any" never waits.
        assert!(
            network_ok(
                TelemetryNetwork::Any,
                &allowed,
                &wifi(WifiState::Disabled, None)
            )
            .is_ok()
        );
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(15 * 60));
        assert_eq!(backoff(2), Duration::from_secs(30 * 60));
        assert_eq!(backoff(3), Duration::from_secs(60 * 60));
        assert_eq!(backoff(5), Duration::from_secs(4 * 3600));
        assert_eq!(backoff(50), Duration::from_secs(4 * 3600));
    }

    /// The whole path, against a stand-in for the service: a closed recording
    /// with a warning is built, sealed, signed, uploaded and recorded, and
    /// what arrives verifies and opens with the service's key.
    #[tokio::test]
    async fn a_recording_with_a_warning_is_contributed_end_to_end() {
        use axum::body::Bytes;
        use axum::extract::{Path as AxumPath, State};
        use axum::http::HeaderMap;
        use axum::routing::{get, post, put};
        use axum::{Json, Router};
        use std::collections::HashMap;
        use telemetry_format::keys::{RecipientPrivateKey, verify_signature};
        use telemetry_format::manifest::{Manifest, SIGNATURE_HEADER, ServerInfo};
        use tokio::io::AsyncWriteExt;

        crate::crypto_provider::install_default();

        // The service.
        let (ingest_sk, ingest_pk) = RecipientPrivateKey::generate();
        #[derive(Default)]
        struct Received {
            manifest: Option<(Vec<u8>, String)>,
            parts: HashMap<String, Vec<u8>>,
            finalized: Option<String>,
        }
        let received = Arc::new(tokio::sync::Mutex::new(Received::default()));
        let info = ServerInfo {
            format: telemetry_format::FORMAT.into(),
            name: "Test Dataset".into(),
            description: None,
            contact: None,
            site_url: None,
            ingest_public_key: ingest_pk.to_base64(),
            archive_public_key: None,
            accepted_tiers: vec![Tier::Summary],
            max_summary_bytes: 64 * 1024 * 1024,
            max_capture_bytes: 0,
        };
        let app = Router::new()
            .route(
                "/.well-known/rayhunter-telemetry",
                get({
                    let info = info.clone();
                    move || async move { Json(info) }
                }),
            )
            .route(
                "/v1/submissions",
                post(
                    |State(r): State<Arc<tokio::sync::Mutex<Received>>>,
                     headers: HeaderMap,
                     body: Bytes| async move {
                        let sig = headers[SIGNATURE_HEADER].to_str().unwrap().to_string();
                        let manifest: Manifest = serde_json::from_slice(&body).unwrap();
                        verify_signature(&manifest.submitter_public_key, &body, &sig).unwrap();
                        manifest.check_shape().unwrap();
                        r.lock().await.manifest = Some((body.to_vec(), sig));
                        Json(serde_json::json!({ "submission_id": manifest.submission_id }))
                    },
                ),
            )
            .route(
                "/v1/submissions/{id}/parts/{name}",
                put(
                    |State(r): State<Arc<tokio::sync::Mutex<Received>>>,
                     AxumPath((_, name)): AxumPath<(String, String)>,
                     body: Bytes| async move {
                        r.lock().await.parts.insert(name, body.to_vec());
                        axum::http::StatusCode::CREATED
                    },
                ),
            )
            .route(
                "/v1/submissions/{id}/finalize",
                post(
                    |State(r): State<Arc<tokio::sync::Mutex<Received>>>,
                     AxumPath(id): AxumPath<String>,
                     headers: HeaderMap| async move {
                        let sig = headers[SIGNATURE_HEADER].to_str().unwrap().to_string();
                        let guard = r.lock().await;
                        let (manifest_bytes, _) = guard.manifest.as_ref().unwrap();
                        let manifest: Manifest = serde_json::from_slice(manifest_bytes).unwrap();
                        verify_signature(
                            &manifest.submitter_public_key,
                            &finalize_message(&id),
                            &sig,
                        )
                        .unwrap();
                        drop(guard);
                        r.lock().await.finalized = Some(id);
                        axum::http::StatusCode::OK
                    },
                ),
            )
            .with_state(received.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // The unit: a store with one closed recording that raised a warning.
        let dir = tempfile::tempdir().unwrap();
        let store_dir = dir.path().join("qmdl");
        let mut store = RecordingStore::create(&store_dir).await.unwrap();
        let (mut qmdl, mut analysis) = store
            .new_entry(crate::config::GpsMode::Disabled)
            .await
            .unwrap();
        qmdl.write_all(b"not really a capture").await.unwrap();
        qmdl.flush().await.unwrap();
        analysis.write_all(concat!(
            r#"{"analyzers":[{"name":"IMSI Requested","description":"d","version":4}],"rayhunter":{"rayhunter_version":"0.12.3","system_os":"linux","arch":"arm"},"report_version":3}"#, "\n",
            r#"{"packet_num":3,"packet_timestamp":"2026-09-02T10:00:00+00:00","skipped_message_reason":null,"events":[{"event_type":"High","message":"Identity requested without authentication"}]}"#, "\n"
        ).as_bytes()).await.unwrap();
        analysis.flush().await.unwrap();
        let name = store.manifest.entries[store.current_entry.unwrap()]
            .name
            .clone();
        store.update_current_entry_qmdl_size(20).await.unwrap();
        store.close_current_entry().await.unwrap();
        let store = Arc::new(RwLock::new(store));

        let config = TelemetryConfig {
            enabled: true,
            server_url: url.clone(),
            ingest_public_key: Some(ingest_pk.to_base64()),
            tier: Tier::Summary,
            network: TelemetryNetwork::Any,
            min_age_secs: 0,
            poll_interval_secs: 30,
            ..TelemetryConfig::default()
        };
        config.validate().unwrap();
        let state = Arc::new(TelemetryState::new(config.clone(), dir.path().join("auth")));
        let deps = WorkerDeps {
            config,
            device: Device::Orbic,
            store: store.clone(),
            wifi_status: Arc::new(RwLock::new(WifiStatus::default())),
            state: state.clone(),
        };
        run_round(&deps).await.unwrap();

        // What arrived.
        let got = received.lock().await;
        let (manifest_bytes, _) = got.manifest.as_ref().expect("a manifest arrived");
        let manifest: Manifest = serde_json::from_slice(manifest_bytes).unwrap();
        assert_eq!(manifest.tier, Tier::Summary);
        assert_eq!(manifest.parts.len(), 1);
        assert_eq!(
            got.finalized.as_deref(),
            Some(manifest.submission_id.as_str())
        );
        let sealed = got
            .parts
            .get("summary.enc")
            .expect("the summary part arrived");
        assert_eq!(sealed.len() as u64, manifest.parts[0].ciphertext_bytes);
        assert_eq!(
            telemetry_format::hex(&<sha2::Sha256 as sha2::Digest>::digest(sealed)),
            manifest.parts[0].sha256
        );
        let mut plain = Vec::new();
        telemetry_format::stream::open(
            &ingest_sk,
            &info_for(&manifest.submission_id, "summary.enc"),
            std::io::Cursor::new(sealed),
            &mut plain,
        )
        .unwrap();
        assert!(plain.starts_with(b"PK\x03\x04"), "the summary is a zip");
        let text = String::from_utf8_lossy(&plain);
        assert!(text.contains("telemetry.json"));
        assert!(text.contains("\"analyzer\": \"IMSI Requested\""));
        assert!(text.contains("redaction-report.json"));
        assert!(!text.contains(".qmdl"), "no raw capture in a summary");
        drop(got);

        // And what the unit recorded.
        let store = store.read().await;
        let (_, entry) = store.entry_for_name(&name).unwrap();
        let submission = entry.telemetry_submission.as_ref().expect("marked as sent");
        assert_eq!(submission.submission_id, manifest.submission_id);
        assert_eq!(submission.server_url, url);
        assert!(
            !store_dir
                .join("telemetry-spool")
                .join(&manifest.submission_id)
                .exists()
        );
        let status = state.status.read().await;
        assert_eq!(status.submitted_count, 1);
        assert!(status.queued.is_empty());
        assert!(status.last_error.is_none(), "{:?}", status.last_error);
        assert_eq!(status.server_name.as_deref(), Some("Test Dataset"));

        // A second round sends nothing: it is already contributed.
        drop(store);
        drop(status);
        run_round(&deps).await.unwrap();
        assert_eq!(state.status.read().await.submitted_count, 1);
    }

    #[test]
    fn a_changed_service_key_is_refused_by_name() {
        let (_, a) = telemetry_format::keys::RecipientPrivateKey::generate();
        let (_, b) = telemetry_format::keys::RecipientPrivateKey::generate();
        assert!(pinned_key(Some(&a.to_base64()), &a.to_base64()).is_ok());
        let err = pinned_key(Some(&a.to_base64()), &b.to_base64()).unwrap_err();
        assert!(
            err.contains(&b.key_id()) && err.contains(&a.key_id()),
            "{err}"
        );
        assert!(pinned_key(None, &a.to_base64()).is_err());
        assert!(pinned_key(Some("junk"), &a.to_base64()).is_err());
    }
}
