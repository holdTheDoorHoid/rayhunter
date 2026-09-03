//! The ingest API.
//!
//! Four requests from units, none of them trusted: the manifest that opens
//! a submission is checked for shape, signature, tier, sizes and recipient
//! keys before a directory is made for it; each part must arrive with the
//! length and hash the manifest promised; finalize and withdraw must be
//! signed by the same key. Limits are per client address and per submitter
//! key, and there is a cap on the whole data directory.
//!
//! What this server holds is deliberately little: manifests, ciphertext,
//! decrypted summaries awaiting review, and the ingest private key. No
//! addresses are written to disk.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64_decode::decode as b64_decode;
use futures::StreamExt;
use log::{info, warn};
use sha2::{Digest, Sha256};
use telemetry_format::keys::{
    RecipientPrivateKey, RecipientPublicKey, key_id_of, verify_signature,
};
use telemetry_format::manifest::{
    Manifest, PartKind, SIGNATURE_HEADER, ServerInfo, Tier, WELL_KNOWN_PATH, WithdrawRequest,
    finalize_message,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};

use crate::ingest;
use crate::store::{self, Record, Status};

/// A tiny base64 decoder for the submitter's public key, to derive its id
/// without another dependency in this crate.
mod base64_decode {
    pub fn decode(text: &str) -> Option<Vec<u8>> {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::with_capacity(text.len() * 3 / 4);
        let mut buffer = 0u32;
        let mut bits = 0u32;
        for byte in text.trim().bytes() {
            if byte == b'=' {
                break;
            }
            let value = TABLE.iter().position(|&c| c == byte)? as u32;
            buffer = (buffer << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
        Some(out)
    }
}

pub struct ServerOptions {
    pub data_dir: PathBuf,
    pub name: String,
    pub description: Option<String>,
    pub contact: Option<String>,
    pub site_url: Option<String>,
    pub ingest_private: RecipientPrivateKey,
    pub ingest_public: RecipientPublicKey,
    pub archive_public: Option<RecipientPublicKey>,
    pub max_summary_bytes: u64,
    pub max_capture_bytes: u64,
    pub behind_proxy: bool,
    pub max_disk_bytes: u64,
}

/// Opens per address and per key, over a sliding day.
#[derive(Default)]
struct Limiter {
    by_ip: HashMap<IpAddr, Vec<Instant>>,
    by_key: HashMap<String, Vec<Instant>>,
}

const PER_IP_PER_HOUR: usize = 30;
const PER_KEY_PER_DAY: usize = 100;

impl Limiter {
    fn allow(&mut self, ip: IpAddr, key_id: &str, now: Instant) -> Result<(), &'static str> {
        let day = Duration::from_secs(24 * 3600);
        let hour = Duration::from_secs(3600);
        for list in self.by_ip.values_mut() {
            list.retain(|t| now.duration_since(*t) < day);
        }
        for list in self.by_key.values_mut() {
            list.retain(|t| now.duration_since(*t) < day);
        }
        self.by_ip.retain(|_, l| !l.is_empty());
        self.by_key.retain(|_, l| !l.is_empty());

        let recent_ip = self
            .by_ip
            .get(&ip)
            .map(|l| l.iter().filter(|t| now.duration_since(**t) < hour).count())
            .unwrap_or(0);
        if recent_ip >= PER_IP_PER_HOUR {
            return Err("too many submissions from this address; try again later");
        }
        let recent_key = self.by_key.get(key_id).map(|l| l.len()).unwrap_or(0);
        if recent_key >= PER_KEY_PER_DAY {
            return Err("too many submissions from this unit today");
        }
        self.by_ip.entry(ip).or_default().push(now);
        self.by_key.entry(key_id.to_string()).or_default().push(now);
        Ok(())
    }
}

pub struct ServerCtx {
    opts: ServerOptions,
    info: ServerInfo,
    limiter: Mutex<Limiter>,
    bans: RwLock<HashSet<String>>,
}

impl ServerCtx {
    pub async fn new(opts: ServerOptions) -> anyhow::Result<Arc<Self>> {
        tokio::fs::create_dir_all(store::submissions_dir(&opts.data_dir)).await?;
        let mut accepted_tiers = vec![Tier::Summary];
        if opts.archive_public.is_some() {
            accepted_tiers.push(Tier::Full);
        }
        let info = ServerInfo {
            format: telemetry_format::FORMAT.to_string(),
            name: opts.name.clone(),
            description: opts.description.clone(),
            contact: opts.contact.clone(),
            site_url: opts.site_url.clone(),
            ingest_public_key: opts.ingest_public.to_base64(),
            archive_public_key: opts.archive_public.as_ref().map(|k| k.to_base64()),
            accepted_tiers,
            max_summary_bytes: opts.max_summary_bytes,
            max_capture_bytes: if opts.archive_public.is_some() {
                opts.max_capture_bytes
            } else {
                0
            },
        };
        let bans = load_bans(&opts.data_dir).await;
        if !bans.is_empty() {
            info!("{} banned submitter keys", bans.len());
        }
        let ctx = Arc::new(ServerCtx {
            opts,
            info,
            limiter: Mutex::new(Limiter::default()),
            bans: RwLock::new(bans),
        });
        collect_garbage(&ctx.opts.data_dir).await;
        Ok(ctx)
    }
}

async fn load_bans(data: &std::path::Path) -> HashSet<String> {
    match tokio::fs::read(data.join("bans.json")).await {
        Ok(bytes) => serde_json::from_slice::<Vec<String>>(&bytes)
            .map(|v| v.into_iter().collect())
            .unwrap_or_else(|e| {
                warn!("bans.json is unreadable, treating it as empty: {e}");
                HashSet::new()
            }),
        Err(_) => HashSet::new(),
    }
}

/// Drop submissions that were opened but never finished.
pub async fn collect_garbage(data: &std::path::Path) {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    let Ok(records) = store::list(data).await else {
        return;
    };
    for record in records {
        if record.status != Status::Pending {
            continue;
        }
        let Ok(received) = chrono::DateTime::parse_from_rfc3339(&record.received_at) else {
            continue;
        };
        if received < cutoff
            && let Some(dir) = store::dir_for(data, &record.submission_id)
        {
            info!("dropping unfinished submission {}", record.submission_id);
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
    }
}

pub fn router(ctx: Arc<ServerCtx>) -> Router {
    Router::new()
        .route(WELL_KNOWN_PATH, get(well_known))
        .route("/v1/submissions", post(open_submission))
        .route("/v1/submissions/{id}/parts/{name}", put(put_part))
        .route("/v1/submissions/{id}/finalize", post(finalize))
        .route("/v1/submissions/{id}/withdraw", post(withdraw))
        .with_state(ctx)
}

pub async fn serve(ctx: Arc<ServerCtx>, bind: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(
        "{} listening on {bind}, ingest key {}{}",
        ctx.info.name,
        ctx.opts.ingest_public.key_id(),
        match &ctx.opts.archive_public {
            Some(k) => format!(", archive key {} (full submissions accepted)", k.key_id()),
            None => ", summary submissions only".to_string(),
        }
    );
    let gc_dir = ctx.opts.data_dir.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(3600));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            collect_garbage(&gc_dir).await;
        }
    });
    axum::serve(
        listener,
        router(ctx).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

type Reply = Result<axum::response::Response, (StatusCode, String)>;

fn bad(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

fn client_ip(ctx: &ServerCtx, headers: &HeaderMap, addr: SocketAddr) -> IpAddr {
    if ctx.opts.behind_proxy
        && let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(last) = forwarded.split(',').next_back()
        && let Ok(ip) = last.trim().parse::<IpAddr>()
    {
        return ip;
    }
    addr.ip()
}

fn signature_of(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .ok_or_else(|| bad(StatusCode::UNAUTHORIZED, "missing signature"))
}

async fn read_limited(body: Body, limit: usize) -> Result<Vec<u8>, (StatusCode, String)> {
    let mut stream = body.into_data_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| bad(StatusCode::BAD_REQUEST, e.to_string()))?;
        if out.len() + chunk.len() > limit {
            return Err(bad(StatusCode::PAYLOAD_TOO_LARGE, "body too large"));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn well_known(State(ctx): State<Arc<ServerCtx>>) -> Json<ServerInfo> {
    Json(ctx.info.clone())
}

#[derive(serde::Serialize)]
struct OpenedResponse {
    submission_id: String,
    parts: Vec<String>,
}

async fn open_submission(
    State(ctx): State<Arc<ServerCtx>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Body,
) -> Reply {
    let ip = client_ip(&ctx, &headers, addr);
    let signature = signature_of(&headers)?;
    let bytes = read_limited(body, 64 * 1024).await?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| bad(StatusCode::BAD_REQUEST, format!("manifest: {e}")))?;
    manifest
        .check_shape()
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e))?;
    verify_signature(&manifest.submitter_public_key, &bytes, &signature)
        .map_err(|_| bad(StatusCode::UNAUTHORIZED, "the signature does not match"))?;
    let key_bytes = b64_decode(&manifest.submitter_public_key)
        .ok_or_else(|| bad(StatusCode::BAD_REQUEST, "unreadable submitter key"))?;
    let key_id = key_id_of(&key_bytes);
    if ctx.bans.read().await.contains(&key_id) {
        return Err(bad(StatusCode::FORBIDDEN, "this unit is not accepted"));
    }
    if !ctx.info.accepted_tiers.contains(&manifest.tier) {
        return Err(bad(
            StatusCode::FORBIDDEN,
            format!("this service does not accept {} submissions", manifest.tier),
        ));
    }
    for part in &manifest.parts {
        let (cap, expected_key) = match part.kind {
            PartKind::Summary => (
                ctx.opts.max_summary_bytes,
                Some(ctx.opts.ingest_public.key_id()),
            ),
            PartKind::Capture => (
                ctx.opts.max_capture_bytes,
                ctx.opts.archive_public.as_ref().map(|k| k.key_id()),
            ),
        };
        if part.ciphertext_bytes > cap || part.ciphertext_bytes == 0 {
            return Err(bad(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "part {} is {} bytes; the limit is {cap}",
                    part.name, part.ciphertext_bytes
                ),
            ));
        }
        if expected_key.as_deref() != Some(part.recipient_key_id.as_str()) {
            return Err(bad(
                StatusCode::BAD_REQUEST,
                format!(
                    "part {} is encrypted to key {}, which this service does not hold",
                    part.name, part.recipient_key_id
                ),
            ));
        }
    }
    if store::disk_usage(&ctx.opts.data_dir).await > ctx.opts.max_disk_bytes {
        return Err(bad(
            StatusCode::INSUFFICIENT_STORAGE,
            "this service is full; try again later",
        ));
    }
    ctx.limiter
        .lock()
        .await
        .allow(ip, &key_id, Instant::now())
        .map_err(|e| bad(StatusCode::TOO_MANY_REQUESTS, e))?;
    if store::load(&ctx.opts.data_dir, &manifest.submission_id)
        .await
        .map_err(internal)?
        .is_some()
    {
        return Err(bad(StatusCode::CONFLICT, "that submission id is taken"));
    }

    let dir = store::dir_for(&ctx.opts.data_dir, &manifest.submission_id)
        .ok_or_else(|| bad(StatusCode::BAD_REQUEST, "bad id"))?;
    tokio::fs::create_dir_all(dir.join("parts"))
        .await
        .map_err(internal)?;
    tokio::fs::write(dir.join("manifest.json"), &bytes)
        .await
        .map_err(internal)?;
    tokio::fs::write(dir.join("manifest.sig"), format!("{signature}\n"))
        .await
        .map_err(internal)?;
    let record = Record {
        submission_id: manifest.submission_id.clone(),
        received_at: store::now(),
        status: Status::Pending,
        tier: manifest.tier,
        submitter_key_id: key_id,
        parts_received: Vec::new(),
        finalized_at: None,
        review: None,
        withdrawn_at: None,
        failure: None,
        max_severity: None,
        warning_count: 0,
    };
    store::save(&ctx.opts.data_dir, &record)
        .await
        .map_err(internal)?;
    info!(
        "opened {} submission {} from unit {}",
        manifest.tier, manifest.submission_id, record.submitter_key_id
    );
    Ok((
        StatusCode::CREATED,
        Json(OpenedResponse {
            submission_id: manifest.submission_id,
            parts: manifest.parts.iter().map(|p| p.name.clone()).collect(),
        }),
    )
        .into_response())
}

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    warn!("internal error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal error".to_string(),
    )
}

async fn load_pending(
    ctx: &ServerCtx,
    id: &str,
) -> Result<(Record, Manifest), (StatusCode, String)> {
    let record = store::load(&ctx.opts.data_dir, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "no such submission"))?;
    if record.status != Status::Pending {
        return Err(bad(
            StatusCode::CONFLICT,
            format!("submission is {}", record.status.as_str()),
        ));
    }
    let manifest = store::parsed_manifest(&ctx.opts.data_dir, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "no such submission"))?;
    Ok((record, manifest))
}

async fn put_part(
    State(ctx): State<Arc<ServerCtx>>,
    AxumPath((id, name)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Reply {
    let (mut record, manifest) = load_pending(&ctx, &id).await?;
    let part = manifest
        .parts
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "the manifest names no such part"))?;
    if record.parts_received.contains(&name) {
        return Err(bad(StatusCode::CONFLICT, "that part has arrived already"));
    }
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or_else(|| bad(StatusCode::LENGTH_REQUIRED, "Content-Length is required"))?;
    if declared != part.ciphertext_bytes {
        return Err(bad(
            StatusCode::BAD_REQUEST,
            format!(
                "Content-Length {declared} but the manifest promised {}",
                part.ciphertext_bytes
            ),
        ));
    }
    let dir = store::dir_for(&ctx.opts.data_dir, &id)
        .ok_or_else(|| bad(StatusCode::BAD_REQUEST, "bad id"))?;
    let final_path = dir.join("parts").join(&name);
    let tmp_path = dir.join("parts").join(format!("{name}.tmp"));
    let mut file = tokio::fs::File::create(&tmp_path).await.map_err(internal)?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    let mut failure: Option<(StatusCode, String)> = None;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(e) => {
                failure = Some(bad(StatusCode::BAD_REQUEST, e.to_string()));
                break;
            }
        };
        written += chunk.len() as u64;
        if written > part.ciphertext_bytes {
            failure = Some(bad(StatusCode::BAD_REQUEST, "more bytes than promised"));
            break;
        }
        hasher.update(&chunk);
        if let Err(e) = file.write_all(&chunk).await {
            failure = Some(internal(e));
            break;
        }
    }
    if failure.is_none() {
        if written != part.ciphertext_bytes {
            failure = Some(bad(StatusCode::BAD_REQUEST, "fewer bytes than promised"));
        } else if telemetry_format::hex(&hasher.finalize()) != part.sha256 {
            failure = Some(bad(
                StatusCode::BAD_REQUEST,
                "the part's hash does not match the manifest",
            ));
        }
    }
    drop(file);
    if let Some(err) = failure {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(err);
    }
    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .map_err(internal)?;
    record.parts_received.push(name.clone());
    store::save(&ctx.opts.data_dir, &record)
        .await
        .map_err(internal)?;
    info!("submission {id}: part {name} arrived ({written} bytes)");
    Ok(StatusCode::CREATED.into_response())
}

async fn finalize(
    State(ctx): State<Arc<ServerCtx>>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Reply {
    let (mut record, manifest) = load_pending(&ctx, &id).await?;
    let signature = signature_of(&headers)?;
    verify_signature(
        &manifest.submitter_public_key,
        &finalize_message(&id),
        &signature,
    )
    .map_err(|_| bad(StatusCode::UNAUTHORIZED, "the signature does not match"))?;
    let missing: Vec<&str> = manifest
        .parts
        .iter()
        .filter(|p| !record.parts_received.contains(&p.name))
        .map(|p| p.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(bad(
            StatusCode::BAD_REQUEST,
            format!("parts not yet received: {}", missing.join(", ")),
        ));
    }
    match ingest::finalize(&ctx.opts.data_dir, &ctx.opts.ingest_private, &id, &manifest).await {
        Ok(summary) => {
            record.status = Status::Received;
            record.finalized_at = Some(store::now());
            record.max_severity = summary.analysis.warnings.max_severity().map(String::from);
            record.warning_count = summary.analysis.warnings.total();
            record.failure = None;
            store::save(&ctx.opts.data_dir, &record)
                .await
                .map_err(internal)?;
            info!(
                "submission {id} received: {} warnings, worst {}",
                record.warning_count,
                record.max_severity.as_deref().unwrap_or("none")
            );
            Ok(Json(serde_json::json!({ "status": "received" })).into_response())
        }
        Err(e) => {
            warn!("submission {id} could not be finalized: {e:#}");
            record.failure = Some(format!("{e:#}"));
            let _ = store::save(&ctx.opts.data_dir, &record).await;
            Err(bad(StatusCode::BAD_REQUEST, format!("{e:#}")))
        }
    }
}

async fn withdraw(
    State(ctx): State<Arc<ServerCtx>>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    body: Body,
) -> Reply {
    let signature = signature_of(&headers)?;
    let bytes = read_limited(body, 4 * 1024).await?;
    let manifest = store::parsed_manifest(&ctx.opts.data_dir, &id)
        .await
        .map_err(internal)?
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "no such submission"))?;
    verify_signature(&manifest.submitter_public_key, &bytes, &signature)
        .map_err(|_| bad(StatusCode::UNAUTHORIZED, "the signature does not match"))?;
    let request: WithdrawRequest = serde_json::from_slice(&bytes)
        .map_err(|e| bad(StatusCode::BAD_REQUEST, format!("request: {e}")))?;
    if request.submission_id != id || request.format != telemetry_format::FORMAT {
        return Err(bad(
            StatusCode::BAD_REQUEST,
            "the request names a different submission",
        ));
    }
    store::withdraw(&ctx.opts.data_dir, &id)
        .await
        .map_err(internal)?;
    info!("submission {id} withdrawn by its unit");
    Ok(Json(serde_json::json!({ "status": "withdrawn" })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_zip::tokio::write::ZipFileWriter;
    use async_zip::{Compression, ZipEntryBuilder};
    use telemetry_format::keys::SubmitterKey;
    use telemetry_format::manifest::{ClientInfo, Consent, PartInfo};
    use telemetry_format::stream::{info_for, seal};
    use telemetry_format::summary::{
        AnalysisMeta, Location, LocationPrecision, Summary, WarningCounts,
    };
    use tokio_util::compat::FuturesAsyncWriteCompatExt;

    struct Service {
        url: String,
        data: std::path::PathBuf,
        ingest_pk: RecipientPublicKey,
        _dir: tempfile::TempDir,
    }

    /// See the dev-dependency note in Cargo.toml: a workspace build hands
    /// reqwest a rustls backend that needs a provider installed first.
    fn install_crypto_provider() {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            let _ = rustls_rustcrypto::provider().install_default();
        });
    }

    async fn start(accept_full: bool) -> Service {
        install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let (ingest_sk, ingest_pk) = RecipientPrivateKey::generate();
        let (_, archive_pk) = RecipientPrivateKey::generate();
        let ctx = ServerCtx::new(ServerOptions {
            data_dir: dir.path().join("data"),
            name: "Test Dataset".into(),
            description: None,
            contact: None,
            site_url: None,
            ingest_private: ingest_sk,
            ingest_public: ingest_pk.clone(),
            archive_public: accept_full.then_some(archive_pk),
            max_summary_bytes: 1024 * 1024,
            max_capture_bytes: 1024 * 1024,
            behind_proxy: false,
            max_disk_bytes: 1024 * 1024 * 1024,
        })
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(
                listener,
                router(ctx).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap()
        });
        Service {
            url,
            data: dir.path().join("data"),
            ingest_pk,
            _dir: dir,
        }
    }

    /// A summary bundle the way a unit would build it, sealed.
    async fn sealed_summary(
        service: &Service,
        id: &str,
        with_location: bool,
    ) -> (Vec<u8>, PartInfo) {
        let summary = Summary {
            format: telemetry_format::FORMAT.into(),
            submission_id: id.into(),
            tier: Some(Tier::Summary),
            analysis: AnalysisMeta {
                warnings: WarningCounts {
                    low: 0,
                    medium: 1,
                    high: 0,
                },
                ..Default::default()
            },
            location: with_location.then_some(Location {
                precision: LocationPrecision::Coarse,
                latitude: 37.8,
                longitude: -122.4,
                source: "gps_api".into(),
                fix_count: 3,
            }),
            contents: vec!["telemetry.json".into(), "x.ndjson".into()],
            ..Default::default()
        };
        let mut zip_bytes = Vec::new();
        {
            let mut zip = ZipFileWriter::with_tokio(&mut zip_bytes);
            for (name, body) in [
                ("x.ndjson", b"{}\n".to_vec()),
                ("telemetry.json", serde_json::to_vec(&summary).unwrap()),
            ] {
                let entry = ZipEntryBuilder::new(name.to_string().into(), Compression::Stored);
                let mut w = zip.write_entry_stream(entry).await.unwrap().compat_write();
                tokio::io::AsyncWriteExt::write_all(&mut w, &body)
                    .await
                    .unwrap();
                w.into_inner().close().await.unwrap();
            }
            zip.close().await.unwrap();
        }
        let mut sealed = Vec::new();
        let info = seal(
            &service.ingest_pk,
            &info_for(id, "summary.enc"),
            std::io::Cursor::new(&zip_bytes),
            &mut sealed,
        )
        .unwrap();
        let part = PartInfo {
            name: "summary.enc".into(),
            kind: PartKind::Summary,
            recipient_key_id: service.ingest_pk.key_id(),
            plaintext_bytes: info.plaintext_bytes,
            ciphertext_bytes: info.ciphertext_bytes,
            sha256: info.sha256,
        };
        (sealed, part)
    }

    fn manifest(id: &str, key: &SubmitterKey, tier: Tier, parts: Vec<PartInfo>) -> Vec<u8> {
        serde_json::to_vec(&Manifest {
            format: telemetry_format::FORMAT.into(),
            submission_id: id.into(),
            created_at: "2026-09-02T12:00:00Z".into(),
            tier,
            submitter_public_key: key.public_key_base64(),
            consent: Consent {
                tier,
                acknowledged_at: (tier == Tier::Full).then(|| "2026-09-01T00:00:00Z".into()),
            },
            client: ClientInfo {
                name: "test".into(),
                version: "0".into(),
            },
            parts,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn a_summary_submission_is_received_reviewed_published_and_withdrawn() {
        let service = start(false).await;
        let client = reqwest::Client::new();
        let key = SubmitterKey::generate();
        let id = telemetry_format::new_submission_id();
        let (sealed, part) = sealed_summary(&service, &id, true).await;

        // The description a unit reads first.
        let info: ServerInfo = client
            .get(format!("{}{}", service.url, WELL_KNOWN_PATH))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(info.accepted_tiers, vec![Tier::Summary]);
        assert!(info.archive_public_key.is_none());

        let body = manifest(&id, &key, Tier::Summary, vec![part.clone()]);
        let response = client
            .post(format!("{}/v1/submissions", service.url))
            .header(SIGNATURE_HEADER, key.sign(&body))
            .body(body.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201, "{}", response.text().await.unwrap());

        // Opening it again is a conflict, and a wrong hash is refused.
        let again = client
            .post(format!("{}/v1/submissions", service.url))
            .header(SIGNATURE_HEADER, key.sign(&body))
            .body(body.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(again.status(), 409);
        let mut corrupt = sealed.clone();
        corrupt[100] ^= 1;
        let response = client
            .put(format!(
                "{}/v1/submissions/{id}/parts/summary.enc",
                service.url
            ))
            .body(corrupt)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        assert!(
            !service
                .data
                .join("submissions")
                .join(&id)
                .join("parts/summary.enc")
                .exists()
        );

        // Finalizing before the part is there is refused.
        let response = client
            .post(format!("{}/v1/submissions/{id}/finalize", service.url))
            .header(SIGNATURE_HEADER, key.sign(&finalize_message(&id)))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);

        let response = client
            .put(format!(
                "{}/v1/submissions/{id}/parts/summary.enc",
                service.url
            ))
            .body(sealed.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201, "{}", response.text().await.unwrap());

        // The wrong key cannot finalize; the right one can.
        let other = SubmitterKey::generate();
        let response = client
            .post(format!("{}/v1/submissions/{id}/finalize", service.url))
            .header(SIGNATURE_HEADER, other.sign(&finalize_message(&id)))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        let response = client
            .post(format!("{}/v1/submissions/{id}/finalize", service.url))
            .header(SIGNATURE_HEADER, key.sign(&finalize_message(&id)))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());

        let record = store::load(&service.data, &id).await.unwrap().unwrap();
        assert_eq!(record.status, Status::Received);
        assert_eq!(record.max_severity.as_deref(), Some("Medium"));
        assert_eq!(record.warning_count, 1);
        let summary = store::load_summary(&service.data, &id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(summary.submission_id, id);

        // Nothing publishes until reviewed.
        let site = service._dir.path().join("site");
        assert_eq!(
            crate::publish::publish(&service.data, &site, "T", None)
                .await
                .unwrap(),
            0
        );
        store::review(
            &service.data,
            &id,
            Status::Verified,
            vec!["interesting".into()],
            Some("note".into()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            crate::publish::publish(&service.data, &site, "T", Some("https://x.example"))
                .await
                .unwrap(),
            1
        );
        let index = std::fs::read_to_string(site.join("index.html")).unwrap();
        assert!(index.contains(&id));
        assert!(index.contains("interesting"));
        assert!(site.join("s").join(&id).join("index.html").exists());
        assert!(site.join("files").join(&id).join("x.ndjson").exists());
        assert!(site.join("files").join(&id).join("summary.zip").exists());
        let geo: serde_json::Value =
            serde_json::from_slice(&std::fs::read(site.join("data/map.geojson")).unwrap()).unwrap();
        assert_eq!(geo["features"].as_array().unwrap().len(), 1);
        let feed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(site.join("data/submissions.json")).unwrap())
                .unwrap();
        assert_eq!(
            feed["submissions"][0]["url"],
            format!("https://x.example/s/{id}/")
        );
        let csv = std::fs::read_to_string(site.join("data/submissions.csv")).unwrap();
        assert_eq!(csv.lines().count(), 2);

        // Withdrawal: only the signing key can, and afterwards the payload
        // is gone and the next publish drops it.
        let request = serde_json::to_vec(&WithdrawRequest {
            format: telemetry_format::FORMAT.into(),
            submission_id: id.clone(),
            requested_at: "2026-09-03T00:00:00Z".into(),
            reason: None,
        })
        .unwrap();
        let response = client
            .post(format!("{}/v1/submissions/{id}/withdraw", service.url))
            .header(SIGNATURE_HEADER, other.sign(&request))
            .body(request.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        let response = client
            .post(format!("{}/v1/submissions/{id}/withdraw", service.url))
            .header(SIGNATURE_HEADER, key.sign(&request))
            .body(request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let record = store::load(&service.data, &id).await.unwrap().unwrap();
        assert_eq!(record.status, Status::Withdrawn);
        assert!(
            !service
                .data
                .join("submissions")
                .join(&id)
                .join("summary.zip")
                .exists()
        );
        assert!(
            !service
                .data
                .join("submissions")
                .join(&id)
                .join("parts")
                .exists()
        );
        let site2 = service._dir.path().join("site2");
        assert_eq!(
            crate::publish::publish(&service.data, &site2, "T", None)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn refusals_come_before_anything_is_written() {
        let service = start(false).await;
        let client = reqwest::Client::new();
        let key = SubmitterKey::generate();
        let id = telemetry_format::new_submission_id();
        let (_, part) = sealed_summary(&service, &id, false).await;

        // A bad signature.
        let body = manifest(&id, &key, Tier::Summary, vec![part.clone()]);
        let response = client
            .post(format!("{}/v1/submissions", service.url))
            .header(SIGNATURE_HEADER, SubmitterKey::generate().sign(&body))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);

        // A tier the service does not accept.
        let capture = PartInfo {
            name: "capture.enc".into(),
            kind: PartKind::Capture,
            recipient_key_id: "0000000000000000".into(),
            ..part.clone()
        };
        let body = manifest(&id, &key, Tier::Full, vec![part.clone(), capture]);
        let response = client
            .post(format!("{}/v1/submissions", service.url))
            .header(SIGNATURE_HEADER, key.sign(&body))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);

        // A part bigger than the cap.
        let mut huge = part.clone();
        huge.ciphertext_bytes = 10 * 1024 * 1024;
        let body = manifest(&id, &key, Tier::Summary, vec![huge]);
        let response = client
            .post(format!("{}/v1/submissions", service.url))
            .header(SIGNATURE_HEADER, key.sign(&body))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 413);

        // A part encrypted to a key this service does not hold.
        let mut stranger = part.clone();
        stranger.recipient_key_id = "ffffffffffffffff".into();
        let body = manifest(&id, &key, Tier::Summary, vec![stranger]);
        let response = client
            .post(format!("{}/v1/submissions", service.url))
            .header(SIGNATURE_HEADER, key.sign(&body))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);

        assert!(store::load(&service.data, &id).await.unwrap().is_none());
        assert!(!service.data.join("submissions").join(&id).exists());
    }

    #[test]
    fn the_limiter_counts_per_address_and_per_key() {
        let mut limiter = Limiter::default();
        let now = Instant::now();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for i in 0..PER_IP_PER_HOUR {
            limiter.allow(ip, &format!("key{i}"), now).unwrap();
        }
        assert!(limiter.allow(ip, "another", now).is_err());
        // An hour later the address is fine again.
        let later = now + Duration::from_secs(3601);
        limiter.allow(ip, "another", later).unwrap();
        // One key across many addresses is capped per day.
        let mut limiter = Limiter::default();
        for i in 0..PER_KEY_PER_DAY {
            let ip: IpAddr = format!("10.0.{}.{}", i / 250, i % 250 + 1).parse().unwrap();
            limiter.allow(ip, "busy", now).unwrap();
        }
        assert!(
            limiter
                .allow("192.168.1.1".parse().unwrap(), "busy", now)
                .is_err()
        );
        assert!(
            limiter
                .allow(
                    "192.168.1.1".parse().unwrap(),
                    "busy",
                    now + Duration::from_secs(24 * 3600 + 1)
                )
                .is_ok()
        );
    }

    #[test]
    fn base64_decoding_matches_the_standard_alphabet() {
        assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(b64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert!(b64_decode("not base64!").is_none());
        let key = SubmitterKey::generate();
        assert_eq!(
            b64_decode(&key.public_key_base64()).unwrap(),
            key.public_key_bytes()
        );
    }
}
