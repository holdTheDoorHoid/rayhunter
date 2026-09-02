use anyhow::Error;
use async_zip::Compression;
use async_zip::ZipEntryBuilder;
use async_zip::tokio::write::ZipFileWriter;
use axum::Json;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::State;
use axum::http::header::{self, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Local};
use futures::TryStreamExt;
use log::{error, warn};
use rayhunter::qmdl::QmdlMessageReader;
use serde::{Deserialize, Serialize};
use std::pin::pin;
use std::sync::Arc;
use tokio::fs::write;
use tokio::io::copy;
use tokio::io::duplex;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::compat::FuturesAsyncWriteCompatExt;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use crate::analysis::{AnalysisCtrlMessage, AnalysisStatus};
use crate::cell_info::{CellInfo, CellTracker};
use crate::config::{Config, GpsMode};
use crate::diag::DiagDeviceCtrlMessage;
use crate::display::DisplayState;
use crate::gps::GpsData;
use crate::notifications::DEFAULT_NOTIFICATION_TIMEOUT;
use crate::pcap::{generate_pcap_data, generate_redacted_pcap_data, load_gps_records_for_entry};
use crate::qmdl_store::{FileKind, RecordingStore};
use crate::update::UpdateStatus;

pub struct ServerState {
    pub config_path: String,
    /// This unit's certificates, when TLS is up. `None` means the plain
    /// port is all there is this run.
    pub tls: Option<Arc<crate::tls::TlsRenewer>>,
    /// Which browsers are trusted, and the setup window.
    pub pairing: Arc<crate::pairing::Pairing>,
    /// The terminal's second gate.
    pub stepup: Arc<crate::stepup::StepUp>,
    pub config: Config,
    /// The accounts as they stand now, rather than as they were at startup.
    ///
    /// `config` is a snapshot taken when the daemon started, so an account
    /// added through the API is not in it. Reading accounts from that snapshot
    /// meant a new one vanished from the settings page on the next reload, and
    /// then — because saving settings rewrites the file from the snapshot —
    /// was erased from the config file entirely by the next save.
    ///
    /// Holding them separately also means a new account takes effect at once,
    /// instead of only after a restart.
    pub web_users: Arc<RwLock<Vec<crate::web_auth::WebUser>>>,
    pub qmdl_store_lock: Arc<RwLock<RecordingStore>>,
    pub diag_device_ctrl_sender: Sender<DiagDeviceCtrlMessage>,
    pub analysis_status_lock: Arc<RwLock<AnalysisStatus>>,
    pub analysis_sender: Sender<AnalysisCtrlMessage>,
    pub daemon_restart_token: CancellationToken,
    pub ui_update_sender: Option<Sender<DisplayState>>,
    /// Shared with the display, so a keypress can be simulated for testing.
    pub suppression: Option<crate::display::SharedSuppression>,
    /// Shared with the display: a full-screen picture, such as a pairing
    /// code, that the server can put up.
    pub display_override: Option<crate::display::SharedOverride>,
    pub cell_tracker: Arc<RwLock<CellTracker>>,
    pub wifi_status: Arc<RwLock<wifi_station::WifiStatus>>,
    pub wifi_scan_lock: tokio::sync::Mutex<()>,
    pub gps_state: Arc<RwLock<Option<GpsData>>>,
    pub update_status_lock: Arc<RwLock<UpdateStatus>>,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/qmdl/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::OK, description = "QMDL download successful", content_type = "application/octet-stream"),
        (status = StatusCode::NOT_FOUND, description = "Could not find file {name}"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "QMDL file is empty, or error opening file")
    ),
    params(
        ("name" = String, Path, description = "QMDL filename to convert and download")
    ),
    summary = "Download a QMDL file",
    description = "Stream the QMDL file {name} to the client."
))]
pub async fn get_qmdl(
    State(state): State<Arc<ServerState>>,
    Path(qmdl_name): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let qmdl_idx = qmdl_name.trim_end_matches(".qmdl");
    let qmdl_store = state.qmdl_store_lock.read().await;
    let (entry_index, _) = qmdl_store.entry_for_name(qmdl_idx).ok_or((
        StatusCode::NOT_FOUND,
        format!("couldn't find qmdl file with name {qmdl_idx}"),
    ))?;
    let qmdl_file = qmdl_store
        .open_file(entry_index, FileKind::Qmdl)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("error opening QMDL file: {err}"),
            )
        })?
        .ok_or((StatusCode::NOT_FOUND, "QMDL file not found".to_string()))?;
    let qmdl_reader = QmdlMessageReader::new(qmdl_file).await.map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("error reading QMDL file: {err}"),
        )
    })?;

    let headers = [(CONTENT_TYPE, "application/octet-stream")];
    let body = Body::from_stream(qmdl_reader.into_qmdl_stream());
    Ok((headers, body).into_response())
}

pub async fn serve_static(
    State(_): State<Arc<ServerState>>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let path = path.trim_start_matches('/');

    match path {
        "rayhunter_orca_only.png" => (
            [(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))],
            include_bytes!("../web/build/rayhunter_orca_only.png"),
        )
            .into_response(),
        "rayhunter_text.png" => (
            [(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))],
            include_bytes!("../web/build/rayhunter_text.png"),
        )
            .into_response(),
        "favicon.png" => (
            [(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))],
            include_bytes!("../web/build/favicon.png"),
        )
            .into_response(),
        "index.html" => (
            [
                (header::CONTENT_TYPE, HeaderValue::from_static("text/html")),
                (header::CONTENT_ENCODING, HeaderValue::from_static("gzip")),
            ],
            include_bytes!("../web/build/index.html.gz"),
        )
            .into_response(),
        "pair.html" => pair_page(),
        path => {
            warn!("404 on path: {path}");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

fn pair_page() -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("text/html")),
            (header::CONTENT_ENCODING, HeaderValue::from_static("gzip")),
            // Never cached: it decides what to show from the unit's state.
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        include_bytes!("../web/build/pair.html.gz"),
    )
        .into_response()
}

/// The pairing page, at `/pair` and at the setup link `/s/<token>`.
///
/// The same page either way; it reads the token, if any, from its own
/// address. The token is deliberately not looked at here: a browser that
/// followed a stale link gets the page and a clear message, not a bare
/// error.
pub async fn serve_pair_page() -> Response {
    pair_page()
}

fn pair_error(e: crate::pairing::PairError) -> (StatusCode, String) {
    use crate::pairing::PairError::*;
    let status = match e {
        AlreadyComplete | NoPassphrase => StatusCode::CONFLICT,
        WindowClosed => StatusCode::GONE,
        WrongToken { .. } | WrongPassphrase => StatusCode::FORBIDDEN,
        PassphraseTooShort => StatusCode::UNPROCESSABLE_ENTITY,
        Backoff(_) => StatusCode::TOO_MANY_REQUESTS,
        NoSuchDevice => StatusCode::NOT_FOUND,
        NoCode | NotPressed => StatusCode::GONE,
        Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, e.to_string())
}

fn stepup_error(e: crate::stepup::StepUpError) -> (StatusCode, String) {
    use crate::stepup::StepUpError::*;
    let status = match e {
        NoPending | Expired | TooManyWrong => StatusCode::GONE,
        WrongCode { .. } | NotYours => StatusCode::FORBIDDEN,
    };
    (status, e.to_string())
}

/// Which browser is asking, for things kept per browser. Loopback has no
/// cookie and gets a fixed name: it is one place, the USB cable.
fn device_id_of(current: &Option<axum::Extension<crate::web_auth::CurrentDevice>>) -> String {
    current
        .as_ref()
        .map(|c| c.0.0.clone())
        .unwrap_or_else(|| "loopback".to_string())
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ChangePassphraseRequest {
    pub current_passphrase: String,
    pub new_passphrase: String,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/passphrase",
    tag = "Pairing",
    request_body(content = ChangePassphraseRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "Changed"),
        (status = StatusCode::FORBIDDEN, description = "The current passphrase is wrong"),
        (status = StatusCode::UNPROCESSABLE_ENTITY, description = "The new passphrase is too short"),
        (status = StatusCode::TOO_MANY_REQUESTS, description = "Too many wrong attempts; wait"),
    ),
    summary = "Change the owner passphrase",
))]
pub async fn change_passphrase(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ChangePassphraseRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .pairing
        .change_passphrase(&req.current_passphrase, &req.new_passphrase)
        .await
        .map_err(pair_error)?;
    Ok(StatusCode::OK)
}

/// A code for adding another browser, and the same as a link and a QR.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PairCodeResponse {
    pub code: String,
    /// `HTTPS://<the host this browser used>/P/<code>`.
    pub url: String,
    pub expires_in_secs: u64,
    /// The link as a QR code, SVG, for the page to show.
    pub svg: String,
}

/// Mint an add-a-device code from a trusted browser.
///
/// The link uses whatever host this browser reached the unit by, so it
/// resolves for the new device on the same network, hotspot or home LAN.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/devices/code",
    tag = "Pairing",
    responses((status = StatusCode::OK, description = "A fresh code", body = PairCodeResponse)),
    summary = "Make a code for adding a device",
))]
pub async fn mint_pair_code(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<PairCodeResponse>, (StatusCode, String)> {
    let (code, ttl) = state.pairing.mint_code().map_err(pair_error)?;
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h))
        .filter(|h| !h.is_empty())
        .unwrap_or("192.168.1.1");
    let url = state
        .pairing
        .code_url(&format!("{host}:{}", state.config.tls_port), &code);
    let svg = crate::display::qr::encode(&url)
        .map(|c| crate::display::qr::svg(&c, 2))
        .unwrap_or_default();
    Ok(Json(PairCodeResponse {
        code,
        url,
        expires_in_secs: ttl.as_secs(),
        svg,
    }))
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PairCodeRequest {
    pub code: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/pair/code",
    tag = "Pairing",
    request_body(content = PairCodeRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "This browser is now trusted; the cookie is in Set-Cookie", body = PairedResponse),
        (status = StatusCode::FORBIDDEN, description = "Wrong code"),
        (status = StatusCode::GONE, description = "No code is active"),
    ),
    summary = "Pair this browser with a code from a trusted one",
))]
pub async fn pair_with_code(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<PairCodeRequest>,
) -> Result<Response, (StatusCode, String)> {
    let issued = state
        .pairing
        .pair_with_code(&req.code, req.device_name.as_deref(), &user_agent(&headers))
        .await
        .map_err(pair_error)?;
    Ok(paired(issued))
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PressRequestResponse {
    pub id: String,
    pub seconds: u64,
}

/// Ask to pair by pressing the unit's button: for units with no screen,
/// and for anyone who cannot scan.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/setup/press-request",
    tag = "Pairing",
    responses(
        (status = StatusCode::OK, description = "Waiting for the button", body = PressRequestResponse),
        (status = StatusCode::CONFLICT, description = "This unit already has an owner"),
    ),
    summary = "Pair by button press: start waiting",
))]
pub async fn request_press(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<PressRequestResponse>, (StatusCode, String)> {
    let id = state.pairing.request_press().await.map_err(pair_error)?;
    Ok(Json(PressRequestResponse {
        id,
        seconds: crate::pairing::PRESS_WINDOW.as_secs(),
    }))
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PressStatusResponse {
    pub approved: bool,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/setup/press-status/{id}",
    tag = "Pairing",
    params(("id" = String, Path, description = "The id from press-request")),
    responses(
        (status = StatusCode::OK, description = "Whether the button has been pressed", body = PressStatusResponse),
        (status = StatusCode::GONE, description = "That request has lapsed"),
    ),
    summary = "Pair by button press: has it been pressed?",
))]
pub async fn press_status(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<PressStatusResponse>, (StatusCode, String)> {
    match state.pairing.press_status(&id) {
        Some(approved) => Ok(Json(PressStatusResponse { approved })),
        None => Err(pair_error(crate::pairing::PairError::NotPressed)),
    }
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct CompleteByPressRequest {
    pub id: String,
    pub passphrase: String,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub browser_unix_ms: Option<i64>,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/setup/complete-press",
    tag = "Pairing",
    request_body(content = CompleteByPressRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "Set up; the cookie is in Set-Cookie", body = PairedResponse),
        (status = StatusCode::GONE, description = "The button was not pressed in time"),
    ),
    summary = "Pair by button press: finish setup",
))]
pub async fn complete_setup_by_press(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CompleteByPressRequest>,
) -> Result<Response, (StatusCode, String)> {
    let issued = state
        .pairing
        .complete_setup_by_press(
            &req.id,
            &req.passphrase,
            req.device_name.as_deref(),
            &user_agent(&headers),
        )
        .await
        .map_err(pair_error)?;
    if let Some(browser_ms) = req.browser_unix_ms {
        let offset =
            chrono::TimeDelta::milliseconds(browser_ms - chrono::Utc::now().timestamp_millis());
        rayhunter::clock::set_offset(offset);
        clock_changed(&state);
    }
    Ok(paired(issued))
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct StepUpStartRequest {
    pub passphrase: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct StepUpResponse {
    /// Whether the unit has a screen to show the code on. Without one, a
    /// button press on the unit confirms instead.
    pub has_screen: bool,
    pub seconds: u64,
}

/// Start a terminal step-up: the passphrase, then a code on the unit.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/stepup/start",
    tag = "Terminal",
    request_body(content = StepUpStartRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "A code is on the unit's screen, or it awaits a button press", body = StepUpResponse),
        (status = StatusCode::FORBIDDEN, description = "Wrong passphrase"),
        (status = StatusCode::TOO_MANY_REQUESTS, description = "Too many wrong attempts; wait"),
    ),
    summary = "Terminal step-up: start",
))]
pub async fn stepup_start(
    State(state): State<Arc<ServerState>>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
    Json(req): Json<StepUpStartRequest>,
) -> Result<Json<StepUpResponse>, (StatusCode, String)> {
    state
        .pairing
        .verify_passphrase(&req.passphrase)
        .await
        .map_err(pair_error)?;
    let ttl = state
        .stepup
        .start(&device_id_of(&current))
        .map_err(stepup_error)?;
    Ok(Json(StepUpResponse {
        has_screen: state.stepup.has_screen(),
        seconds: ttl.as_secs(),
    }))
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct StepUpConfirmRequest {
    pub code: String,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/stepup/confirm",
    tag = "Terminal",
    request_body(content = StepUpConfirmRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "The terminal is open for a while", body = StepUpResponse),
        (status = StatusCode::FORBIDDEN, description = "Wrong code"),
        (status = StatusCode::GONE, description = "No code is waiting, or too many wrong ones"),
    ),
    summary = "Terminal step-up: confirm the code",
))]
pub async fn stepup_confirm(
    State(state): State<Arc<ServerState>>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
    Json(req): Json<StepUpConfirmRequest>,
) -> Result<Json<StepUpResponse>, (StatusCode, String)> {
    let window = state
        .stepup
        .confirm(&device_id_of(&current), &req.code)
        .map_err(stepup_error)?;
    Ok(Json(StepUpResponse {
        has_screen: state.stepup.has_screen(),
        seconds: window.as_secs(),
    }))
}

/// Close this browser's terminal window now rather than letting it lapse.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/stepup/end",
    tag = "Terminal",
    responses((status = StatusCode::OK, description = "Closed")),
    summary = "Terminal step-up: end",
))]
pub async fn stepup_end(
    State(state): State<Arc<ServerState>>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
) -> StatusCode {
    state.stepup.end(&device_id_of(&current));
    StatusCode::OK
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct StepUpStatus {
    pub active: bool,
    pub seconds_left: u64,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/stepup/status",
    tag = "Terminal",
    responses((status = StatusCode::OK, description = "Whether this browser's terminal window is open", body = StepUpStatus)),
    summary = "Terminal step-up: status",
))]
pub async fn stepup_status(
    State(state): State<Arc<ServerState>>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
) -> Json<StepUpStatus> {
    let id = device_id_of(&current);
    let active = state.stepup.active(&id);
    Json(StepUpStatus {
        active,
        seconds_left: state
            .stepup
            .remaining(&id)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn user_agent(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// Where a unit is in its life: fresh, mid-setup, or owned.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct SetupStatus {
    /// Somebody owns this unit.
    pub setup_complete: bool,
    /// The code is on the screen and a token will be accepted.
    pub window_open: bool,
    pub seconds_left: u64,
    pub paired_devices: usize,
    /// Whether "pair with the passphrase" can work.
    pub has_passphrase: bool,
    /// Whether "sign in with an existing account" can work.
    pub has_accounts: bool,
    pub tls_port: u16,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/setup/status",
    tag = "Pairing",
    responses((status = StatusCode::OK, description = "Where setup stands", body = SetupStatus)),
    summary = "Where this unit is in its setup",
))]
pub async fn get_setup_status(State(state): State<Arc<ServerState>>) -> Json<SetupStatus> {
    let window = state.pairing.setup_window();
    Json(SetupStatus {
        setup_complete: state.pairing.setup_complete().await,
        window_open: window.is_some(),
        seconds_left: window.map(|(_, left)| left.as_secs()).unwrap_or(0),
        paired_devices: state.pairing.device_count().await,
        has_passphrase: state.pairing.has_passphrase().await,
        has_accounts: !state.web_users.read().await.is_empty(),
        tls_port: state.config.tls_port,
    })
}

/// First contact with a new unit.
#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct SetupCompleteRequest {
    /// The token from the screen, as scanned or as typed.
    pub token: String,
    /// The owner passphrase to set. At least eight characters.
    pub passphrase: String,
    /// What to call this browser. Made up from the user agent if absent.
    #[serde(default)]
    pub device_name: Option<String>,
    /// The browser's clock, so the unit can set its own from it.
    #[serde(default)]
    pub browser_unix_ms: Option<i64>,
}

/// What a browser gets back once it is trusted. The cookie travels in the
/// `Set-Cookie` header, not here.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PairedResponse {
    pub device_id: String,
    pub name: String,
}

fn paired(issued: crate::pairing::IssuedDevice) -> Response {
    (
        StatusCode::OK,
        [(header::SET_COOKIE, issued.set_cookie_header())],
        Json(PairedResponse {
            device_id: issued.id,
            name: issued.name,
        }),
    )
        .into_response()
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/setup/complete",
    tag = "Pairing",
    request_body(content = SetupCompleteRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "This browser is now the first trusted device; the cookie is in Set-Cookie", body = PairedResponse),
        (status = StatusCode::FORBIDDEN, description = "Wrong token"),
        (status = StatusCode::GONE, description = "The setup window is closed"),
        (status = StatusCode::CONFLICT, description = "Setup is already complete"),
        (status = StatusCode::UNPROCESSABLE_ENTITY, description = "The passphrase is too short"),
    ),
    summary = "Complete first-time setup",
))]
pub async fn complete_setup(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetupCompleteRequest>,
) -> Result<Response, (StatusCode, String)> {
    let issued = state
        .pairing
        .complete_setup(
            &req.token,
            &req.passphrase,
            req.device_name.as_deref(),
            &user_agent(&headers),
        )
        .await
        .map_err(pair_error)?;
    // The phone knows what time it is and the unit very likely does not.
    if let Some(browser_ms) = req.browser_unix_ms {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let offset = chrono::TimeDelta::milliseconds(browser_ms - now_ms);
        rayhunter::clock::set_offset(offset);
        clock_changed(&state);
        log::info!(
            "clock offset set from the setup browser: {}s",
            offset.num_seconds()
        );
    }
    Ok(paired(issued))
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PairPassphraseRequest {
    pub passphrase: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/pair/passphrase",
    tag = "Pairing",
    request_body(content = PairPassphraseRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "This browser is now trusted; the cookie is in Set-Cookie", body = PairedResponse),
        (status = StatusCode::FORBIDDEN, description = "Wrong passphrase"),
        (status = StatusCode::TOO_MANY_REQUESTS, description = "Too many wrong attempts; wait"),
        (status = StatusCode::CONFLICT, description = "No passphrase has been set"),
    ),
    summary = "Pair this browser with the owner passphrase",
))]
pub async fn pair_with_passphrase(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<PairPassphraseRequest>,
) -> Result<Response, (StatusCode, String)> {
    let issued = state
        .pairing
        .pair_with_passphrase(
            &req.passphrase,
            req.device_name.as_deref(),
            &user_agent(&headers),
        )
        .await
        .map_err(pair_error)?;
    Ok(paired(issued))
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PairAccountRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

/// For units that had web accounts before pairing existed.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/pair/account",
    tag = "Pairing",
    request_body(content = PairAccountRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "This browser is now trusted; the cookie is in Set-Cookie", body = PairedResponse),
        (status = StatusCode::FORBIDDEN, description = "Wrong username or password"),
        (status = StatusCode::TOO_MANY_REQUESTS, description = "Too many wrong attempts; wait"),
    ),
    summary = "Pair this browser with a web account from before pairing",
))]
pub async fn pair_with_account(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<PairAccountRequest>,
) -> Result<Response, (StatusCode, String)> {
    let users = state.web_users.read().await.clone();
    let issued = state
        .pairing
        .pair_with_account(
            &users,
            &req.username,
            &req.password,
            req.device_name.as_deref(),
            &user_agent(&headers),
        )
        .await
        .map_err(pair_error)?;
    Ok(paired(issued))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/devices",
    tag = "Pairing",
    responses((status = StatusCode::OK, description = "Every trusted browser", body = Vec<crate::pairing::DeviceInfo>)),
    summary = "List trusted devices",
))]
pub async fn list_devices(
    State(state): State<Arc<ServerState>>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
) -> Json<Vec<crate::pairing::DeviceInfo>> {
    let current_id = current.as_ref().map(|c| c.0.0.as_str());
    Json(state.pairing.devices(current_id).await)
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct RenameDeviceRequest {
    pub name: String,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/devices/{id}/rename",
    tag = "Pairing",
    request_body(content = RenameDeviceRequest, content_type = "application/json"),
    params(("id" = String, Path, description = "Device id")),
    responses(
        (status = StatusCode::OK, description = "Renamed"),
        (status = StatusCode::NOT_FOUND, description = "No such device, or an empty name"),
    ),
    summary = "Rename a trusted device",
))]
pub async fn rename_device(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<RenameDeviceRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .pairing
        .rename_device(&id, &req.name)
        .await
        .map_err(pair_error)?;
    Ok(StatusCode::OK)
}

/// Forget a trusted device. Its cookie stops working at once; revoking the
/// browser making the request also clears its cookie, so it lands on the
/// pairing page rather than on a wall of errors.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/devices/{id}/revoke",
    tag = "Pairing",
    params(("id" = String, Path, description = "Device id")),
    responses(
        (status = StatusCode::OK, description = "Revoked"),
        (status = StatusCode::NOT_FOUND, description = "No such device"),
    ),
    summary = "Revoke a trusted device",
))]
pub async fn revoke_device(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
) -> Result<Response, (StatusCode, String)> {
    state.pairing.revoke_device(&id).await.map_err(pair_error)?;
    let is_current = current.map(|c| c.0.0 == id).unwrap_or(false);
    if is_current {
        return Ok((
            StatusCode::OK,
            [(header::SET_COOKIE, crate::pairing::clear_cookie_header())],
            "revoked this browser",
        )
            .into_response());
    }
    Ok((StatusCode::OK, "revoked").into_response())
}

/// Inject a synthetic, clearly labelled warning, for demonstrating Rayhunter.
///
/// The message is fed into the diag stream ahead of analysis, so it is written
/// to the recording and passes through the real detectors rather than being
/// faked further down. That is what makes a demo show how Rayhunter actually
/// works instead of just painting a warning on the screen.
///
/// Refused unless demo_mode is on in the config. A fake surveillance detection
/// is the sort of thing that gets screenshotted and passed off as real, so it
/// should never be a single unguarded request away.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/demo-warning",
    tag = "Demo",
    responses(
        (status = StatusCode::ACCEPTED, description = "Demo warning injected"),
        (status = StatusCode::FORBIDDEN, description = "Demo mode is not enabled"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "Recording is not running"),
    ),
    summary = "Inject a demo warning",
))]
pub async fn trigger_demo_warning(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if !state.config.demo_mode {
        return Err((
            StatusCode::FORBIDDEN,
            "demo mode is not enabled; switch it on in the configuration first".to_string(),
        ));
    }

    // Without a recording there is nothing to inject into, and the warning
    // would vanish with no explanation.
    let recording = {
        let store = state.qmdl_store_lock.read().await;
        store.current_entry.is_some()
    };
    if !recording {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "start a recording first: the demo warning is written into it".to_string(),
        ));
    }

    state
        .diag_device_ctrl_sender
        .send(DiagDeviceCtrlMessage::InjectDemo)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to reach the recording thread".to_string(),
            )
        })?;

    Ok((
        StatusCode::ACCEPTED,
        "injected a demo warning; it will appear in the history shortly".to_string(),
    ))
}

/// What the radio currently sees on the air.
///
/// Only updates while a recording is running, since it comes from the modem
/// diagnostic stream. `has_data` is false before anything has arrived, so the
/// UI can say why it is empty rather than appearing broken.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/cell-info",
    tag = "Cell information",
    responses(
        (status = StatusCode::OK, description = "Success", body = CellInfo)
    ),
    summary = "Get cell information",
    description = "The serving cell, the neighbours the modem can hear, and the cells seen during this run."
))]
pub async fn get_cell_info(State(state): State<Arc<ServerState>>) -> Json<CellInfo> {
    let tracker = state.cell_tracker.read().await;
    let mut info = tracker.snapshot();
    // Attached only when the operator asked for it. This endpoint needs no
    // credentials, so on a hotspot it is readable by anyone on the WiFi, and an
    // IMSI is precisely what an IMSI catcher is trying to collect. Off by
    // default means the default build cannot be turned into one.
    if state.config.show_subscriber_identity {
        // Sent even when empty, so the interface can say that nothing has been
        // seen yet and why. Omitting it left a setting that appeared to do
        // nothing at all, which is worse than not offering it.
        info.identities = Some(tracker.identities().unwrap_or_default());
    }
    Json(info)
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/config",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "Success", body = Config)
    ),
    summary = "Get config",
    description = "Show the running configuration for Rayhunter."
))]
pub async fn get_config(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Config>, (StatusCode, String)> {
    let mut config = state.config.clone();
    config.wifi_password = None;
    // The WebDAV password is a stored secret; never round-trip it through the
    // browser. The client learns only whether one is set (a non-empty username
    // or a configured URL implies it), and set_config preserves the stored
    // password when this comes back blank.
    config.webdav.password = None;
    // From the live list, not the startup snapshot, so an account added a
    // moment ago is still here on the next reload.
    config.web_users = state.web_users.read().await.clone();
    // The account names are useful to show; the hashes are not, and serving
    // them would hand an attacker something to grind offline at their leisure.
    for user in config.web_users.iter_mut() {
        user.password_hash = String::new();
    }
    Ok(Json(config))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/config",
    tag = "Configuration",
    request_body(
        content = Option<[Config]>,
        description = "Any or all configuration elements from the valid config schema to be altered may be passed. Invalid keys will be discarded. Invalid values or value types will return an error."
    ),
    responses(
        (status = StatusCode::ACCEPTED, description = "Success"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to parse or write config file"),
        (status = 422, description = "Failed to deserialize JSON body")
    ),
    summary = "Set config",
    description = "Write a new configuration for Rayhunter and trigger a restart."
))]
pub async fn set_config(
    State(state): State<Arc<ServerState>>,
    Json(mut config): Json<Config>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if config.gps_mode != GpsMode::Fixed {
        config.gps_fixed_latitude = None;
        config.gps_fixed_longitude = None;
    }
    // Reject out-of-range durations before anything is written. A value that a
    // runtime constructor cannot represent would otherwise be persisted and
    // then crash the daemon on the next start, over and over.
    if let Err(msg) = config.webdav.validate() {
        return Err((StatusCode::BAD_REQUEST, msg));
    }
    let mut config_to_write = config.clone();
    // Accounts are never taken from the request. The hashes are redacted on
    // the way out, so a client saving the settings page would otherwise post
    // blanks back and lock everybody out of the device. Taken from the live
    // list rather than the startup snapshot, or saving any setting at all
    // would wipe every account added since the daemon started.
    config_to_write.web_users = state.web_users.read().await.clone();
    // Same for the terminal, which by design can only be switched on when
    // flashing and must never be turnable on from the interface itself.
    config_to_write.terminal_enabled = state.config.terminal_enabled;
    // The WebDAV password is redacted in get_config, so a client saving the
    // settings page posts it back blank. Preserve the stored one rather than
    // wiping it; a genuine change sends a non-empty value.
    if config_to_write.webdav.password.is_none() {
        config_to_write.webdav.password = state.config.webdav.password.clone();
    }
    config_to_write.wifi_ssid = None;
    config_to_write.wifi_password = None;
    config_to_write.wifi_security = None;

    let config_str = toml::to_string_pretty(&config_to_write).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to serialize config as TOML: {err}"),
        )
    })?;

    write_config_atomically(&state.config_path, &config_str)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to write config file: {err}"),
            )
        })?;

    wifi_station::update_wpa_conf(&config.wifi_config()).await;

    // Trigger daemon restart after writing config
    state.daemon_restart_token.cancel();
    Ok((
        StatusCode::ACCEPTED,
        "wrote config and triggered restart".to_string(),
    ))
}

/// Largest GIF we accept, in bytes. These devices have only tens of megabytes
/// of RAM free, and the decoder expands each frame to roughly 64KB at 128x128,
/// so accepting arbitrarily large uploads risks pushing the daemon out of
/// memory at playback time.
pub const MAX_GIF_BYTES: usize = 2 * 1024 * 1024;

use crate::display::generic_framebuffer::{MAX_GIF_DIMENSION, image_dimensions, image_kind};

/// Store a GIF for one display state.
///
/// This only writes the file. Recording it in `display_gifs` is left to the
/// ordinary config save, so uploading several GIFs doesn't restart the daemon
/// once per file and drop the connection mid-sequence.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/display-gif/{state}",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "GIF stored; save the config to apply it"),
        (status = StatusCode::BAD_REQUEST, description = "Unknown state, not a GIF, or too large"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to write the GIF"),
    ),
    summary = "Upload a display GIF",
    description = "Upload a GIF to play for one display state when ui_level is CustomGif."
))]
pub async fn set_display_gif(
    State(state): State<Arc<ServerState>>,
    Path(display_state): Path<String>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if !crate::config::DISPLAY_STATE_KEYS.contains(&display_state.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "unknown display state {display_state:?}, expected one of {:?}",
                crate::config::DISPLAY_STATE_KEYS
            ),
        ));
    }
    if body.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "no image data received".to_string(),
        ));
    }
    if body.len() > MAX_GIF_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "image is {} bytes, which is over the {MAX_GIF_BYTES} byte limit",
                body.len()
            ),
        ));
    }
    // Decided from the file's own first bytes, not its name or the content type
    // the browser guessed, both of which the uploader controls.
    if image_kind(&body).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "file is neither a GIF nor a PNG. An animated GIF plays as an animation; a PNG is \
             shown as a still picture."
                .to_string(),
        ));
    }

    // Size on disk says nothing about size in memory, so check the declared
    // canvas before this ever reaches a decoder.
    match image_dimensions(&body) {
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                "image is truncated: no dimensions in the header".to_string(),
            ));
        }
        Some((width, height)) => {
            if width == 0 || height == 0 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("image declares an empty canvas ({width} by {height})"),
                ));
            }
            if width > MAX_GIF_DIMENSION as u32 || height > MAX_GIF_DIMENSION as u32 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "image is {width} by {height} pixels, over the {MAX_GIF_DIMENSION} pixel \
                         limit. A large canvas expands enormously in memory when drawn, however \
                         small the file is, and this device has very little to spare. The screen \
                         is 128 pixels square, so resize it before uploading."
                    ),
                ));
            }
        }
    }

    let dir = &state.config.gif_store_path;
    tokio::fs::create_dir_all(dir).await.map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create GIF directory {dir}: {err}"),
        )
    })?;

    let path = crate::display::generic_framebuffer::gif_path(dir, &display_state);
    write(&path, &body).await.map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to write GIF to {path}: {err}"),
        )
    })?;

    Ok((StatusCode::OK, format!("stored image for {display_state}")))
}

/// Serve back the GIF stored for one display state, so the web UI can preview
/// what is actually on the device rather than only what was uploaded this
/// session.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/display-gif/{state}",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "The stored GIF", content_type = "image/gif"),
        (status = StatusCode::BAD_REQUEST, description = "Unknown display state"),
        (status = StatusCode::NOT_FOUND, description = "No GIF stored for this state"),
    ),
    summary = "Download a display GIF",
))]
pub async fn get_display_gif(
    State(state): State<Arc<ServerState>>,
    Path(display_state): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    // Only known state names are accepted, so the filename can never be
    // steered outside the GIF directory.
    if !crate::config::DISPLAY_STATE_KEYS.contains(&display_state.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown display state {display_state:?}"),
        ));
    }

    let path =
        crate::display::generic_framebuffer::gif_path(&state.config.gif_store_path, &display_state);
    let bytes = tokio::fs::read(&path).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            (
                StatusCode::NOT_FOUND,
                format!("no image stored for {display_state}"),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to read {path}: {err}"),
            )
        }
    })?;

    // Declared from the stored bytes rather than the file name, which still
    // ends in .gif for every state whatever was uploaded.
    let content_type = match image_kind(&bytes) {
        Some(crate::display::generic_framebuffer::ImageKind::Still) => "image/png",
        _ => "image/gif",
    };

    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            // This serves bytes somebody uploaded, from the same origin as the
            // web UI. A file can be a valid image and valid HTML at once, so
            // forbid content sniffing: without this a browser that second
            // guessed the declared type could run it as a page with full
            // access to the API.
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
            // The URL is stable per state, so a replaced GIF would otherwise
            // keep showing the old one from cache.
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache, must-revalidate"),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Remove the GIF for one display state, reverting it to the colored line.
/// As with upload, the config is updated by the ordinary config save.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/display-gif/{state}/delete",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "GIF removed; save the config to apply it"),
        (status = StatusCode::BAD_REQUEST, description = "Unknown display state"),
    ),
    summary = "Delete a display GIF",
))]
pub async fn delete_display_gif(
    State(state): State<Arc<ServerState>>,
    Path(display_state): Path<String>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if !crate::config::DISPLAY_STATE_KEYS.contains(&display_state.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown display state {display_state:?}"),
        ));
    }

    let path =
        crate::display::generic_framebuffer::gif_path(&state.config.gif_store_path, &display_state);
    // A missing file is fine: we only need to end up with no GIF for this state.
    if let Err(err) = tokio::fs::remove_file(&path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        warn!("failed to remove {path}: {err}");
    }

    Ok((StatusCode::OK, format!("removed GIF for {display_state}")))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/test-notification",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "Success"),
        (status = StatusCode::BAD_REQUEST, description = "No notification URL set"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to send HTTP request. Ensure your device can reach the internet.")
    ),
    summary = "Test ntfy notification",
    description = "Send a test notification to the ntfy_url in the running configuration for Rayhunter."
))]
pub async fn test_notification(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    let url = state.config.ntfy_url.as_ref().ok_or((
        StatusCode::BAD_REQUEST,
        "No notification URL configured".to_string(),
    ))?;

    if url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Notification URL is empty".to_string(),
        ));
    }

    let http_client = crate::http_client::client().map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create HTTP client: {err}"),
        )
    })?;
    let message = "Test notification from Rayhunter".to_string();

    crate::notifications::send_notification(
        &http_client,
        url,
        message,
        DEFAULT_NOTIFICATION_TIMEOUT,
    )
    .await
    .map(|()| {
        (
            StatusCode::OK,
            "Test notification sent successfully".to_string(),
        )
    })
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to send test notification: {e}"),
        )
    })
}

/// Response for GET /api/time
#[derive(Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct TimeResponse {
    /// The raw system time (without clock offset)
    #[cfg_attr(feature = "apidocs", schema(value_type = String))]
    pub system_time: DateTime<Local>,
    /// The adjusted time (system time + offset)
    #[cfg_attr(feature = "apidocs", schema(value_type = String))]
    pub adjusted_time: DateTime<Local>,
    /// The current offset in seconds
    pub offset_seconds: i64,
}

/// Request for POST /api/time-offset
#[derive(Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct SetTimeOffsetRequest {
    /// The offset to set, in seconds
    pub offset_seconds: i64,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/time",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "Success", body = TimeResponse)
    ),
    summary = "Get time",
    description = "Get the current time and offset (in seconds) of the device."
))]
pub async fn get_time() -> Json<TimeResponse> {
    let system_time = Local::now();
    let adjusted_time = rayhunter::clock::get_adjusted_now();
    let offset_seconds = adjusted_time
        .signed_duration_since(system_time)
        .num_seconds();
    Json(TimeResponse {
        system_time,
        adjusted_time,
        offset_seconds,
    })
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/time-offset",
    tag = "Configuration",
    request_body(
        content = SetTimeOffsetRequest
    ),
    responses(
        (status = StatusCode::OK, description = "Success", body = TimeResponse)
    ),
    summary = "Set time offset",
    description = "Set the difference (in seconds) between the system time and the adjusted time for Rayhunter."
))]
pub async fn set_time_offset(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SetTimeOffsetRequest>,
) -> StatusCode {
    rayhunter::clock::set_offset(chrono::TimeDelta::seconds(req.offset_seconds));
    clock_changed(&state);
    StatusCode::OK
}

/// The detectors that ran over a recording, from the first line of its
/// analysis file.
///
/// Reads one line rather than the file, which on a long recording is large.
/// Any failure returns `None`: the sidecar says nothing about analysis rather
/// than the download failing.
async fn read_analysis_header(
    qmdl_store_lock: &Arc<RwLock<RecordingStore>>,
    entry_index: usize,
) -> Option<crate::export_metadata::AnalysisInfo> {
    use tokio::io::AsyncBufReadExt;

    let file = {
        let store = qmdl_store_lock.read().await;
        store
            .open_file(entry_index, FileKind::Analysis)
            .await
            .ok()??
    };
    let mut lines = tokio::io::BufReader::new(file).lines();
    let first = lines.next_line().await.ok()??;
    crate::export_metadata::analysis_info_from_first_line(&first)
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/zip/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::OK, description = "ZIP download successful. It is possible that if the PCAP fails to convert, the same status will be returned, but the file will contain only the QMDL file.", content_type = "application/zip"),
        (status = StatusCode::NOT_FOUND, description = "Could not find file {name}"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "QMDL file is empty, or error opening file")
    ),
    params(
        ("name" = String, Path, description = "QMDL filename to convert and download")
    ),
    summary = "Download a ZIP file",
    description = "Stream a ZIP file to the client which contains the QMDL file {name}, its NDJSON analysis report, its GPS NDJSON file (if present), and a PCAP generated from the QMDL."
))]
pub async fn get_zip(
    State(state): State<Arc<ServerState>>,
    Path(entry_name): Path<String>,
    axum::extract::RawQuery(raw_query): axum::extract::RawQuery,
) -> Result<Response, (StatusCode, String)> {
    // `?redact=1` asks for a bundle meant to be shared: the device's own
    // identifiers removed from the capture, and the raw QMDL left out, since
    // nothing removes them from that. Parsed by hand for the same reason the
    // packet list is: axum's Query extractor needs a feature this build does
    // not enable.
    let redact = raw_query
        .as_deref()
        .map(|q| {
            q.split('&')
                .any(|pair| matches!(pair, "redact=1" | "redact=true"))
        })
        .unwrap_or(false);
    let qmdl_idx = entry_name.trim_end_matches(".zip").to_owned();
    let (entry_index, download_name, manifest_entry) = {
        let qmdl_store = state.qmdl_store_lock.read().await;
        let (entry_index, entry) = qmdl_store.entry_for_name(&qmdl_idx).ok_or((
            StatusCode::NOT_FOUND,
            format!("couldn't find entry with name {qmdl_idx}"),
        ))?;

        if entry.qmdl_size_bytes == 0 {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "QMDL file is empty, try again in a bit!".to_string(),
            ));
        }

        (entry_index, entry.display_name.clone(), entry.clone())
    };

    let qmdl_store_lock = state.qmdl_store_lock.clone();
    let gps_records = load_gps_records_for_entry(&state, entry_index).await;
    let device = state.config.device.clone();
    // Kept for the download filename, since the zip writing task below takes
    // ownership of `qmdl_idx`.
    let entry_id = qmdl_idx.clone();

    let (reader, writer) = duplex(8192);

    tokio::spawn(async move {
        let result: Result<(), Error> = async {
            let mut zip = ZipFileWriter::with_tokio(writer);
            let mut redaction_report: Option<crate::redact::RedactionReport> = None;

            // Add stored files. The raw capture is left out of a redacted
            // bundle: redaction happens on the way into the PCAP, and nothing
            // removes identifiers from the QMDL, so including it would hand
            // over exactly what the bundle claims to have removed.
            for &file_kind in FileKind::ALL {
                if redact && file_kind == FileKind::Qmdl {
                    continue;
                }
                let file_opt = {
                    let qmdl_store = qmdl_store_lock.read().await;
                    qmdl_store.open_file(entry_index, file_kind).await?
                };

                let Some(mut file) = file_opt else {
                    continue;
                };

                /*
                 * `qmdl_compressed` is always false here because even if the
                 * QMDL was already compressed, we decompress it before zipping.
                 * This is for two reasons
                 * 1. If this is the current entry, it's still being written and
                 *    lacks a GZIP footer. If we zipped up this partial .gz
                 *    file, some software might consider it damaged and refuse to
                 *    extract it.
                 * 2. Zipping an already-GZIP'd file is redundant and
                 *    inconvenient for the user.
                 */
                let zip_entry = ZipEntryBuilder::new(
                    file_kind.get_filename(&qmdl_idx, false).into(),
                    Compression::Stored,
                );
                // FuturesAsyncWriteCompatExt::compat_write because async-zip's entrystream does
                // not impl tokio's AsyncWrite, but only future's AsyncWrite. This can be removed
                // once https://github.com/Majored/rs-async-zip/pull/160 is released.
                let mut entry_writer = zip.write_entry_stream(zip_entry).await?.compat_write();

                if file_kind == FileKind::Qmdl {
                    let reader = QmdlMessageReader::new(&mut file).await?;
                    let stream = reader.into_qmdl_stream();
                    let mut reader = pin!(stream.into_async_read().compat());
                    copy(&mut reader, &mut entry_writer).await?;
                } else {
                    copy(&mut file, &mut entry_writer).await?;
                }
                entry_writer.into_inner().close().await?;
            }

            // Add PCAP file
            {
                let entry =
                    ZipEntryBuilder::new(format!("{qmdl_idx}.pcapng").into(), Compression::Stored);
                let mut entry_writer = zip.write_entry_stream(entry).await?.compat_write();

                let qmdl_file_for_pcap = {
                    let qmdl_store = qmdl_store_lock.read().await;
                    qmdl_store
                        .open_file(entry_index, FileKind::Qmdl)
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("QMDL file not found"))?
                };
                let qmdl_reader = QmdlMessageReader::new(qmdl_file_for_pcap).await?;

                if redact {
                    match generate_redacted_pcap_data(&mut entry_writer, qmdl_reader, gps_records)
                        .await
                    {
                        Ok(report) => {
                            warn!(
                                "redacted export of {qmdl_idx}: removed {} identifiers from {} NAS messages",
                                report.total(),
                                report.messages_scanned
                            );
                            redaction_report = Some(report);
                        }
                        Err(e) => {
                            // A redacted bundle whose PCAP failed to build has
                            // nothing left in it, and must not look like a
                            // successful redaction of an empty capture.
                            error!("Failed to generate redacted PCAP: {e:?}");
                            return Err(e);
                        }
                    }
                } else if let Err(e) =
                    generate_pcap_data(&mut entry_writer, qmdl_reader, gps_records).await
                {
                    // if we fail to generate the PCAP file, we should still continue and give the
                    // user the QMDL.
                    error!("Failed to generate PCAP: {e:?}");
                }

                entry_writer.into_inner().close().await?;
            }

            // The sidecar goes in last, on purpose: everything above is the
            // capture, and a problem building a description of it must never
            // cost somebody the thing being described.
            {
                let analysis = read_analysis_header(&qmdl_store_lock, entry_index).await;
                let metadata = crate::export_metadata::build(
                    &manifest_entry,
                    &device,
                    analysis,
                    chrono::Local::now(),
                );
                match serde_json::to_vec_pretty(&metadata) {
                    Ok(json) => {
                        let entry = ZipEntryBuilder::new(
                            "metadata.json".to_string().into(),
                            Compression::Stored,
                        );
                        let mut entry_writer = zip.write_entry_stream(entry).await?.compat_write();
                        tokio::io::AsyncWriteExt::write_all(&mut entry_writer, &json).await?;
                        entry_writer.into_inner().close().await?;
                    }
                    Err(err) => error!("failed to build metadata.json: {err:?}"),
                }
            }

            // What was removed, said plainly. A redacted bundle that does not
            // say what it took out invites the reader to assume it took out
            // everything, which is a promise this cannot make.
            if let Some(report) = &redaction_report {
                match serde_json::to_vec_pretty(report) {
                    Ok(json) => {
                        let entry = ZipEntryBuilder::new(
                            "redaction-report.json".to_string().into(),
                            Compression::Stored,
                        );
                        let mut entry_writer =
                            zip.write_entry_stream(entry).await?.compat_write();
                        tokio::io::AsyncWriteExt::write_all(&mut entry_writer, &json).await?;
                        entry_writer.into_inner().close().await?;
                    }
                    Err(err) => error!("failed to build redaction-report.json: {err:?}"),
                }
            }

            zip.close().await?;
            Ok(())
        }
        .await;

        if let Err(e) = result {
            error!("Error generating ZIP file: {e:?}");
        }
    });

    // Name the download after whatever the person called this recording, which
    // is the point of EFForg/rayhunter#501: a folder of timestamps tells you
    // nothing about which recording was which. The timestamp stays on the end
    // so two recordings with the same name never collide, and the name has
    // already been reduced to letters, digits, dash and underscore on the way
    // in, so it cannot smuggle a quote or a path separator into this header.
    let suffix = if redact { "-redacted" } else { "" };
    let filename = match &download_name {
        Some(name) => format!("{name}-{entry_id}{suffix}.zip"),
        None => format!("{entry_id}{suffix}.zip"),
    };
    let headers = [
        (CONTENT_TYPE, "application/zip".to_string()),
        (
            CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ),
    ];
    let body = Body::from_stream(ReaderStream::new(reader));
    Ok((headers, body).into_response())
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/wifi-status",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "Success", body = wifi_station::WifiStatus)
    ),
    summary = "Get wifi status",
    description = "Show the status of the wifi client."
))]
pub async fn get_wifi_status(
    State(state): State<Arc<ServerState>>,
) -> Json<wifi_station::WifiStatus> {
    let status = state.wifi_status.read().await;
    Json(status.clone())
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/wifi-scan",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "Scan success", body = inline(Vec<wifi_station::WifiNetwork>), content_type = "application/json"),
        (status = StatusCode::TOO_MANY_REQUESTS, description = "Scan already in progress"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Scan failed"),
    ),
    summary = "Wifi SSID scan",
    description = "Poll for a list of available wifi networks. Returns an array of WifiNetwork objects."
))]
pub async fn scan_wifi(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<wifi_station::WifiNetwork>>, (StatusCode, String)> {
    let _guard = state.wifi_scan_lock.try_lock().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "WiFi scan already in progress".to_string(),
        )
    })?;
    let networks = wifi_station::scan_wifi_networks(wifi_station::STA_IFACE)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("WiFi scan failed: {e}"),
            )
        })?;
    Ok(Json(networks))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/debug/display-state",
    tag = "Configuration",
    request_body(
        content = DisplayState
    ),
    responses(
        (status = StatusCode::OK, description = "Display state updated successfully"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Error sending update to the display"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "Display system not available")
    ),
    summary = "Set display state",
    description = "Change the display state (color bar or otherwise) of the device for debugging purposes."
))]
pub async fn debug_set_display_state(
    State(state): State<Arc<ServerState>>,
    Json(display_state): Json<DisplayState>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if let Some(ui_sender) = &state.ui_update_sender {
        ui_sender.send(display_state).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to send display state update".to_string(),
            )
        })?;
        Ok((
            StatusCode::OK,
            "display state updated successfully".to_string(),
        ))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "display system not available".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::config::GpsMode;
    use async_zip::base::read::mem::ZipFileReader;
    use axum::extract::{Path, State};
    use futures::AsyncReadExt;
    use rayhunter::{
        diag::{DataType, HdlcEncapsulatedMessage, Message, MessagesContainer},
        qmdl::{QmdlMessageReader, QmdlWriter},
    };
    use tempfile::TempDir;

    async fn create_test_qmdl_store() -> (TempDir, Arc<RwLock<crate::qmdl_store::RecordingStore>>) {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().to_path_buf();
        let store = crate::qmdl_store::RecordingStore::create(&store_path)
            .await
            .unwrap();
        (temp_dir, Arc::new(RwLock::new(store)))
    }

    async fn create_test_entry_with_data(
        store_lock: &Arc<RwLock<crate::qmdl_store::RecordingStore>>,
        test_data: &MessagesContainer,
    ) -> String {
        let entry_name = {
            let mut store = store_lock.write().await;
            let (mut qmdl_gz_file, _analysis_file) =
                store.new_entry(GpsMode::Disabled).await.unwrap();

            let mut writer = QmdlWriter::new(&mut qmdl_gz_file);
            writer.write_container(test_data).await.unwrap();
            writer.close().await.unwrap();

            let qmdl_file_size = qmdl_gz_file.metadata().await.unwrap().len() as usize;

            let current_entry = store.current_entry.unwrap();
            let entry = &store.manifest.entries[current_entry];
            let entry_name = entry.name.clone();

            store
                .update_current_entry_qmdl_size(qmdl_file_size)
                .await
                .unwrap();
            entry_name
        };

        let mut store = store_lock.write().await;
        store.close_current_entry().await.unwrap();
        entry_name
    }

    fn create_test_server_state(
        store_lock: Arc<RwLock<crate::qmdl_store::RecordingStore>>,
    ) -> Arc<ServerState> {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let (analysis_tx, _analysis_rx) = tokio::sync::mpsc::channel(1);

        let analysis_status = {
            let store = store_lock.try_read().unwrap();
            crate::analysis::AnalysisStatus::new(&store)
        };

        Arc::new(ServerState {
            tls: None,
            pairing: Arc::new(crate::pairing::Pairing::ephemeral(
                crate::pairing::AuthState::default(),
                None,
            )),
            stepup: Arc::new(crate::stepup::StepUp::new(None)),
            config_path: "/tmp/test_config.toml".to_string(),
            config: Config::default(),
            web_users: Arc::new(RwLock::new(Vec::new())),
            qmdl_store_lock: store_lock,
            diag_device_ctrl_sender: tx,
            analysis_status_lock: Arc::new(RwLock::new(analysis_status)),
            analysis_sender: analysis_tx,
            daemon_restart_token: CancellationToken::new(),
            ui_update_sender: None,
            suppression: None,
            display_override: None,
            cell_tracker: Arc::new(RwLock::new(CellTracker::new())),
            wifi_status: Arc::new(RwLock::new(wifi_station::WifiStatus::default())),
            wifi_scan_lock: tokio::sync::Mutex::new(()),
            gps_state: Arc::new(RwLock::new(None)),
            update_status_lock: Arc::new(RwLock::new(UpdateStatus::default())),
        })
    }

    // valid HDLC encapsulated diag message generated from
    // rayhunter::diag::test::get_test_message
    fn create_test_container() -> MessagesContainer {
        MessagesContainer {
            data_type: DataType::UserSpace,
            num_messages: 1,
            messages: vec![HdlcEncapsulatedMessage {
                len: 39,
                data: vec![
                    16, 0, 32, 0, 32, 0, 192, 176, 26, 165, 245, 135, 118, 35, 2, 1, 20, 14, 48, 0,
                    160, 0, 2, 8, 0, 0, 217, 15, 5, 0, 0, 0, 0, 1, 0, 10, 13, 196, 126,
                ],
            }],
        }
    }

    #[tokio::test]
    async fn test_get_zip_success() {
        let (_temp_dir, store_lock) = create_test_qmdl_store().await;
        let test_qmdl_data = create_test_container();
        let entry_name = create_test_entry_with_data(&store_lock, &test_qmdl_data).await;
        let state = create_test_server_state(store_lock);

        let response = get_zip(
            State(state),
            Path(entry_name.clone()),
            axum::extract::RawQuery(None),
        )
        .await
        .unwrap();

        let headers = response.headers();
        assert_eq!(headers.get("content-type").unwrap(), "application/zip");

        let body = response.into_body();
        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();

        let zip_reader = ZipFileReader::new(body_bytes.to_vec()).await.unwrap();
        let zip_reader_file = zip_reader.file();
        let filenames: Vec<String> = zip_reader_file
            .entries()
            .iter()
            .map(|entry| entry.filename().as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            filenames,
            vec![
                format!("{entry_name}.qmdl"),
                format!("{entry_name}.ndjson"),
                format!("{entry_name}-gps.ndjson"),
                format!("{entry_name}.pcapng"),
                "metadata.json".to_string(),
            ]
        );

        let mut qmdl_body = Vec::with_capacity(128);
        zip_reader
            .reader_without_entry(0)
            .await
            .unwrap()
            .read_to_end(&mut qmdl_body)
            .await
            .unwrap();
        let mut qmdl_reader = QmdlMessageReader::new(Cursor::new(qmdl_body))
            .await
            .unwrap();
        let expected_message = Message::from_hdlc(&test_qmdl_data.messages[0].data).unwrap();
        assert_eq!(
            qmdl_reader.get_next_message().await.unwrap(),
            Some(Ok(expected_message)),
        );

        // The sidecar has to describe this recording, not merely exist. Its
        // whole value is what it says, and a field quietly going missing would
        // not fail anything at run time.
        let mut metadata_body = Vec::new();
        zip_reader
            .reader_without_entry(4)
            .await
            .unwrap()
            .read_to_end(&mut metadata_body)
            .await
            .unwrap();
        let metadata: crate::export_metadata::RecordingMetadata =
            serde_json::from_slice(&metadata_body).expect("metadata.json parses");
        assert_eq!(metadata.recording.id, entry_name);
        assert_eq!(
            metadata.metadata_version,
            crate::export_metadata::METADATA_VERSION
        );
        assert!(!metadata.rayhunter.version_at_export.is_empty());
    }

    /// A redacted bundle must not contain the raw capture. Redaction happens on
    /// the way into the PCAP, and nothing removes identifiers from the QMDL, so
    /// shipping it would hand over exactly what the bundle claims to have taken
    /// out. This is the assertion that stops that regressing.
    #[tokio::test]
    async fn a_redacted_bundle_leaves_out_the_raw_capture() {
        let (_temp_dir, store_lock) = create_test_qmdl_store().await;
        let test_qmdl_data = create_test_container();
        let entry_name = create_test_entry_with_data(&store_lock, &test_qmdl_data).await;
        let state = create_test_server_state(store_lock);

        let response = get_zip(
            State(state),
            Path(entry_name.clone()),
            axum::extract::RawQuery(Some("redact=1".to_string())),
        )
        .await
        .unwrap();

        // The name has to say so too, or the two bundles are indistinguishable
        // in a downloads folder.
        let disposition = response
            .headers()
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(disposition.contains("-redacted.zip"), "{disposition}");

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let zip_reader = ZipFileReader::new(body_bytes.to_vec()).await.unwrap();
        let filenames: Vec<String> = zip_reader
            .file()
            .entries()
            .iter()
            .map(|entry| entry.filename().as_str().unwrap().to_string())
            .collect();

        assert!(
            !filenames.iter().any(|f| f.ends_with(".qmdl")),
            "the raw capture is in a redacted bundle: {filenames:?}"
        );
        assert!(
            filenames.contains(&"redaction-report.json".to_string()),
            "a redacted bundle must say what it removed: {filenames:?}"
        );
        assert!(
            filenames.iter().any(|f| f.ends_with(".pcapng")),
            "{filenames:?}"
        );
        assert!(
            filenames.contains(&"metadata.json".to_string()),
            "{filenames:?}"
        );
    }

    /// The cap may fall inside a multi-byte character, where String::truncate
    /// panics. The firmware profile turns any panic into a daemon abort, so
    /// terminal output must never be cut mid-character.
    #[test]
    fn terminal_output_is_cut_on_a_character_boundary() {
        let short = truncate_output("hello".to_string());
        assert_eq!(short, "hello");

        // Two-byte characters at even offsets, and again shifted onto odd
        // offsets by a one-byte prefix. Whatever the cap's parity, one of the
        // two puts a character astride it.
        let aligned = "é".repeat(TERMINAL_MAX_OUTPUT);
        let shifted = format!("a{}", "é".repeat(TERMINAL_MAX_OUTPUT));
        for text in [aligned, shifted] {
            let cut = truncate_output(text);
            assert!(cut.len() <= TERMINAL_MAX_OUTPUT + "\n[output truncated]".len());
            assert!(cut.ends_with("\n[output truncated]"));
        }
    }
}

/// A display name and notes for one recording, as sent by the web UI.
#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct AnnotationRequest {
    /// A short label shown instead of the timestamp. Empty clears it.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Free text about the circumstances. Empty clears it.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Name a recording and write notes about it.
///
/// Addresses EFForg/rayhunter#501: recordings are named by the second they
/// started, which says nothing about why anyone made them.
///
/// Stored in the manifest rather than inside the capture. A recording is
/// evidence, and renaming it should never rewrite it.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/annotate-recording/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::ACCEPTED, description = "Saved"),
        (status = StatusCode::BAD_REQUEST, description = "Name or notes too long"),
        (status = StatusCode::NOT_FOUND, description = "No such recording"),
    ),
    summary = "Name a recording",
    description = "Set or clear the display name and notes for one recording."
))]
pub async fn annotate_recording(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    Json(body): Json<AnnotationRequest>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    use crate::qmdl_store::{MAX_DISPLAY_NAME, MAX_NOTES, sanitize_display_name};

    // An empty field means "clear this", which is how the UI removes a name
    // without needing a second endpoint.
    let display_name = match body.display_name.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => {
            if raw.chars().count() > MAX_DISPLAY_NAME {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "name is {} characters, over the {MAX_DISPLAY_NAME} character limit",
                        raw.chars().count()
                    ),
                ));
            }
            let cleaned = sanitize_display_name(raw);
            if cleaned.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "name has no letters, digits, dashes or underscores in it".to_string(),
                ));
            }
            Some(cleaned)
        }
    };

    let notes = match body.notes.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => {
            if raw.chars().count() > MAX_NOTES {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "notes are {} characters, over the {MAX_NOTES} character limit",
                        raw.chars().count()
                    ),
                ));
            }
            Some(raw.to_string())
        }
    };

    let mut store = state.qmdl_store_lock.write().await;
    store
        .set_entry_annotations(&name, display_name, notes)
        .await
        .map_err(|e| match e {
            crate::qmdl_store::RecordingStoreError::NoSuchEntryError => {
                (StatusCode::NOT_FOUND, format!("no recording called {name}"))
            }
            other => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("couldn't save: {other}"),
            ),
        })?;

    Ok((StatusCode::ACCEPTED, "saved".to_string()))
}

/// Pretend a button was pressed, so the step aside behaviour can be tested.
///
/// The real trigger is a physical button, read from the input device. That
/// makes the behaviour impossible to check from a script, which is how it
/// shipped drawing the wrong line height: the logic was reviewed rather than
/// measured. This makes the screen observable instead.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/debug/keypress",
    tag = "Debug",
    responses(
        (status = StatusCode::OK, description = "Display asked to step aside"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "Display system not available"),
    ),
    summary = "Simulate a button press",
    description = "Hold the full screen display back briefly, as a button press does."
))]
pub async fn debug_keypress(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    let Some(suppression) = &state.suppression else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "display system not available".to_string(),
        ));
    };
    suppression.suppress_for(crate::display::KEYPRESS_QUIET_PERIOD);
    Ok((
        StatusCode::OK,
        format!(
            "stepping aside for {} seconds",
            crate::display::KEYPRESS_QUIET_PERIOD.as_secs()
        ),
    ))
}
/// What a person needs to check they are talking to their own unit.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct TlsInfo {
    /// The port the interface is served on over TLS.
    pub port: u16,
    /// SHA-256 of the certificate, `AB:CD:…`, as browsers show it.
    pub fingerprint_sha256: String,
    /// The names and addresses the certificate is for.
    pub subject_alt_names: Vec<String>,
    /// The server certificate itself, PEM.
    pub certificate_pem: String,
    /// The unit's certificate authority: what a person installs to stop
    /// the browser warning for good. Its fingerprint, its name as a phone
    /// lists it, and the certificate as PEM.
    pub ca_fingerprint_sha256: String,
    pub ca_name: String,
    pub ca_pem: String,
    /// When the server certificate runs out, RFC 3339. It is reissued
    /// before then; a browser that trusts the authority never notices.
    pub leaf_not_after: Option<String>,
}

/// Describe this unit's TLS certificate.
///
/// The fingerprint is the one thing a person can compare between what the
/// browser shows and what the unit itself says, so it is available to
/// anyone who can reach the interface at all.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/tls-info",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "The unit's certificate", body = TlsInfo),
        (status = StatusCode::NOT_FOUND, description = "TLS is not available on this unit"),
    ),
    summary = "Describe the TLS certificate",
))]
pub async fn get_tls_info(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<TlsInfo>, (StatusCode, String)> {
    let Some(tls) = &state.tls else {
        return Err((
            StatusCode::NOT_FOUND,
            "TLS is not available on this unit".to_string(),
        ));
    };
    let identity = tls.identity().await;
    Ok(Json(TlsInfo {
        port: state.config.tls_port,
        fingerprint_sha256: identity.fingerprint_hex(),
        subject_alt_names: identity.subject_alt_names(),
        certificate_pem: identity.certificate_pem(),
        ca_fingerprint_sha256: identity.ca_fingerprint_hex(),
        ca_name: identity.ca_name(),
        ca_pem: identity.ca_pem(),
        leaf_not_after: identity.leaf_not_after(),
    }))
}

async fn ca_file(
    state: &ServerState,
    content_type: &'static str,
    filename: &str,
    body: impl FnOnce(&crate::tls::TlsIdentity) -> Vec<u8>,
) -> Result<Response, (StatusCode, String)> {
    let Some(tls) = &state.tls else {
        return Err((
            StatusCode::NOT_FOUND,
            "TLS is not available on this unit".to_string(),
        ));
    };
    let identity = tls.identity().await;
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body(&identity),
    )
        .into_response())
}

/// The authority as PEM: what Android, Windows, Linux and Firefox import.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/ca.pem",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "The unit's certificate authority, PEM", content_type = "application/x-pem-file"),
        (status = StatusCode::NOT_FOUND, description = "TLS is not available on this unit"),
    ),
    summary = "Download the certificate authority (PEM)",
))]
pub async fn get_ca_pem(
    State(state): State<Arc<ServerState>>,
) -> Result<Response, (StatusCode, String)> {
    ca_file(&state, "application/x-pem-file", "rayhunter-ca.pem", |id| {
        id.ca_pem().into_bytes()
    })
    .await
}

/// The authority as DER, for the installers that want that.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/ca.crt",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "The unit's certificate authority, DER", content_type = "application/x-x509-ca-cert"),
        (status = StatusCode::NOT_FOUND, description = "TLS is not available on this unit"),
    ),
    summary = "Download the certificate authority (DER)",
))]
pub async fn get_ca_der(
    State(state): State<Arc<ServerState>>,
) -> Result<Response, (StatusCode, String)> {
    ca_file(
        &state,
        "application/x-x509-ca-cert",
        "rayhunter-ca.crt",
        |id| id.ca_der().to_vec(),
    )
    .await
}

/// The authority as an Apple configuration profile: one tap on an iPhone,
/// iPad or Mac, then a confirmation in Settings.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/ca.mobileconfig",
    tag = "Configuration",
    responses(
        (status = StatusCode::OK, description = "An Apple configuration profile installing the authority", content_type = "application/x-apple-aspen-config"),
        (status = StatusCode::NOT_FOUND, description = "TLS is not available on this unit"),
    ),
    summary = "Download the certificate authority (Apple profile)",
))]
pub async fn get_ca_mobileconfig(
    State(state): State<Arc<ServerState>>,
) -> Result<Response, (StatusCode, String)> {
    ca_file(
        &state,
        "application/x-apple-aspen-config",
        "rayhunter.mobileconfig",
        |id| id.mobileconfig().into_bytes(),
    )
    .await
}

/// The unit has just learnt the time; a server certificate issued without
/// it may be due for replacement. Done in the background so the request
/// that brought the time is not held up.
fn clock_changed(state: &Arc<ServerState>) {
    if let Some(tls) = state.tls.clone() {
        tokio::spawn(async move {
            tls.check().await;
        });
    }
}

fn default_qr_module_px() -> u32 {
    4
}

fn default_qr_seconds() -> u64 {
    600
}

/// Longest a picture may be asked to stay up. Long enough for any test,
/// short enough that a forgotten request does not hide the status line for
/// the rest of the day.
const MAX_QR_SECONDS: u64 = 3600;

/// Longest text a code is made from. The screen limits this far more
/// tightly; this only stops a request from asking for a great deal of work
/// that could never be drawn.
const MAX_QR_TEXT_LEN: usize = 512;

/// A QR code to show on the device's own screen.
#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct DebugQrRequest {
    /// What the code says. Uppercase keeps a URL in the compact
    /// alphanumeric mode; the text is encoded exactly as given.
    pub text: String,
    /// Pixels per module. Reduced automatically if the code will not fit.
    #[serde(default = "default_qr_module_px")]
    pub module_px: u32,
    /// A line of text under the code, for anyone who cannot scan it.
    #[serde(default)]
    pub caption: Option<String>,
    /// How many times larger than the five by seven font to draw the caption.
    #[serde(default)]
    pub caption_scale: Option<u32>,
    /// How long to leave it up.
    #[serde(default = "default_qr_seconds")]
    pub seconds: u64,
}

/// Where the code ended up, so a test can read the screen back and check.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct DebugQrResponse {
    /// QR version: 1 is 21 modules a side, each version adds four.
    pub version: u8,
    /// Modules a side.
    pub size: u32,
    /// Pixels per module actually drawn.
    pub module_px: u32,
    /// White margin around the code, in pixels.
    pub quiet_px: u32,
    /// Top left pixel of the first module.
    pub code_x: u32,
    pub code_y: u32,
    /// Top row of the caption, if one was drawn.
    pub caption_y: Option<u32>,
    pub seconds: u64,
}

/// Show a QR code on the device's own screen for a while.
///
/// This is the drawing half of setup mode, exposed on its own so the thing
/// the whole pairing design rests on, whether a phone can read a code off a
/// screen this small, can be tested before anything is built on top of it.
/// The code replaces everything else on the screen, status line included,
/// and the panel is held awake while it is up.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/debug/qr",
    tag = "Debug",
    request_body(content = DebugQrRequest, content_type = "application/json"),
    responses(
        (status = StatusCode::OK, description = "Code is on the screen", body = DebugQrResponse),
        (status = StatusCode::UNPROCESSABLE_ENTITY, description = "The text cannot be encoded, or the code does not fit the screen"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "This device has no screen to draw on"),
    ),
    summary = "Show a QR code on the device screen",
))]
pub async fn debug_show_qr(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<DebugQrRequest>,
) -> Result<Json<DebugQrResponse>, (StatusCode, String)> {
    use crate::display::qr;

    let Some(override_) = &state.display_override else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "display system not available".to_string(),
        ));
    };
    let Some(geo) = qr::screen_geometry(&state.config.device) else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "this device has no screen a code can be drawn on".to_string(),
        ));
    };
    if req.text.is_empty() || req.text.len() > MAX_QR_TEXT_LEN {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("text must be between 1 and {MAX_QR_TEXT_LEN} bytes"),
        ));
    }
    let Some(code) = qr::encode(&req.text) else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "that text cannot be encoded as a QR code".to_string(),
        ));
    };
    let size = code.size() as u32;
    let caption = req.caption.as_deref().filter(|c| !c.is_empty());
    let Some(layout) = qr::layout(
        size,
        req.module_px,
        caption.is_some(),
        req.caption_scale.unwrap_or(1),
        geo,
    ) else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "a {size} by {size} module code does not fit a {} by {} screen even at {} pixels per module",
                geo.width,
                geo.height,
                qr::MIN_MODULE_PX
            ),
        ));
    };
    let pixels = qr::render(&code, layout, caption, geo);
    let seconds = req.seconds.clamp(1, MAX_QR_SECONDS);
    override_.show(pixels, std::time::Duration::from_secs(seconds));
    log::info!(
        "showing a version {} QR code at {} px per module for {seconds}s",
        code.version().value(),
        layout.module_px
    );
    Ok(Json(DebugQrResponse {
        version: code.version().value(),
        size,
        module_px: layout.module_px,
        quiet_px: layout.quiet_px,
        code_x: layout.code_x,
        code_y: layout.code_y,
        caption_y: layout.caption_y,
        seconds,
    }))
}

/// Take a picture put up by `debug_show_qr` down early.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/debug/qr/clear",
    tag = "Debug",
    responses(
        (status = StatusCode::OK, description = "Nothing is showing any more"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "This device has no screen"),
    ),
    summary = "Clear a QR code from the device screen",
))]
pub async fn debug_clear_qr(
    State(state): State<Arc<ServerState>>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    let Some(override_) = &state.display_override else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "display system not available".to_string(),
        ));
    };
    let message = match override_.remaining() {
        Some(left) => format!("cleared, with {}s left to run", left.as_secs()),
        None => "nothing was showing".to_string(),
    };
    override_.clear();
    Ok((StatusCode::OK, message))
}

/// One account to add or replace.
#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct WebUserRequest {
    pub username: String,
    pub password: String,
}

use crate::web_auth::{MAX_PASSWORD_LEN, MAX_USERNAME_LEN};

/// Serializes account changes so two overlapping requests cannot each read the
/// same list, edit their own copy, and have the last write win — which silently
/// dropped one of the changes. Held across the whole read-modify-write.
static WEB_USER_MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Add a web interface account, or change an existing one's password.
///
/// The password is hashed here and the plaintext is never written anywhere.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/web-users",
    tag = "Configuration",
    request_body(
        content = WebUserRequest,
        description = "The account to add, or whose password to change. The password is hashed on the device and the plaintext is never stored."
    ),
    responses(
        (status = StatusCode::ACCEPTED, description = "Account saved; restart to apply"),
        (status = StatusCode::BAD_REQUEST, description = "Empty username or password"),
    ),
    summary = "Add or update a web interface account",
))]
pub async fn set_web_user(
    State(state): State<Arc<ServerState>>,
    Json(body): Json<WebUserRequest>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    let username = body.username.trim().to_string();
    if username.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "username is empty".to_string()));
    }
    if username.len() > MAX_USERNAME_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("username is too long (limit {MAX_USERNAME_LEN} bytes)"),
        ));
    }
    // Short enough to guess is the same as no password at all, and somebody
    // setting one here believes they are protecting something.
    if body.password.chars().count() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters".to_string(),
        ));
    }
    if body.password.len() > MAX_PASSWORD_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("password is too long (limit {MAX_PASSWORD_LEN} bytes)"),
        ));
    }

    // Serialize the read-modify-write against any other account change.
    let _guard = WEB_USER_MUTATION.lock().await;
    let mut users = state.web_users.read().await.clone();
    // Hashing is deliberately expensive; run it off the single-threaded async
    // runtime so it does not stall every other request while it grinds.
    let password = body.password.clone();
    let hash = tokio::task::spawn_blocking(move || crate::web_auth::hash_password(&password))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to hash password: {e}"),
            )
        })?;
    match users.iter_mut().find(|u| u.username == username) {
        Some(existing) => existing.password_hash = hash,
        None => users.push(crate::web_auth::WebUser {
            username: username.clone(),
            password_hash: hash,
        }),
    }

    write_web_users(&state, users).await?;
    Ok((StatusCode::ACCEPTED, format!("saved {username}")))
}

/// Remove a web interface account.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/web-users/{username}/delete",
    tag = "Configuration",
    responses(
        (status = StatusCode::ACCEPTED, description = "Account removed"),
        (status = StatusCode::NOT_FOUND, description = "No such account"),
    ),
    summary = "Remove a web interface account",
))]
pub async fn delete_web_user(
    State(state): State<Arc<ServerState>>,
    Path(username): Path<String>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    // Serialize against any other account change, so a concurrent add is not
    // silently undone by this delete's write.
    let _guard = WEB_USER_MUTATION.lock().await;
    let mut users = state.web_users.read().await.clone();
    let before = users.len();
    users.retain(|u| u.username != username);
    if users.len() == before {
        return Err((StatusCode::NOT_FOUND, format!("no account {username}")));
    }
    // Removing the last account is allowed: with no account the terminal
    // refuses to run (see run_terminal_command), so this returns the device to
    // the open, terminal-disabled state rather than opening a root shell.
    write_web_users(&state, users).await?;
    Ok((StatusCode::ACCEPTED, format!("removed {username}")))
}

/// Write the account list back to the config file, leaving everything else as
/// the running daemon has it.
async fn write_web_users(
    state: &Arc<ServerState>,
    users: Vec<crate::web_auth::WebUser>,
) -> Result<(), (StatusCode, String)> {
    let mut config = state.config.clone();
    config.web_users = users.clone();
    config.wifi_ssid = None;
    config.wifi_password = None;
    config.wifi_security = None;

    let config_str = toml::to_string_pretty(&config).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to serialize config: {err}"),
        )
    })?;
    write_config_atomically(&state.config_path, &config_str)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to write config: {err}"),
            )
        })?;
    // Only after the write succeeded, so a failed write does not start
    // demanding a password that was never saved.
    *state.web_users.write().await = users;
    Ok(())
}

/// Write the config file so an interruption leaves either the old contents or
/// the new contents intact, never a half-written file. The config parser fails
/// on invalid TOML and the daemon will not start without it, so a torn write
/// during a settings save could otherwise brick the device until the file is
/// repaired by hand. Write a sibling temp file, flush it to disk, then rename
/// over the target — a rename within one filesystem is atomic.
async fn write_config_atomically(path: &str, contents: &str) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let target = std::path::Path::new(path);
    let Some(name) = target.file_name() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "config path has no file name",
        ));
    };
    let mut tmp_name = name.to_os_string();
    tmp_name.push(".new");
    let tmp = target.with_file_name(tmp_name);

    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(contents.as_bytes()).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&tmp, target).await?;
    // Best effort: flush the directory entry so the rename itself survives a
    // power loss. Not fatal if the platform will not let us.
    if let Some(parent) = target.parent()
        && let Ok(dir) = tokio::fs::File::open(parent).await
    {
        let _ = dir.sync_all().await;
    }
    Ok(())
}

/// A command to run on the device.
#[derive(Debug, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct TerminalRequest {
    pub command: String,
}

/// What running it produced.
#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct TerminalResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// True when the command was still running when its time ran out.
    pub timed_out: bool,
}

/// Longest a command may run before it is killed.
///
/// Without this a command that waits forever holds a task and a shell open on
/// a device with very little of either to spare.
const TERMINAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Most output returned. Enough for a directory listing or a log tail, far
/// short of what it takes to exhaust memory on this hardware.
const TERMINAL_MAX_OUTPUT: usize = 256 * 1024;

/// Longest command string accepted. A command line is short; anything past this
/// is not a command but a way to make the daemon hold a large string.
const TERMINAL_MAX_COMMAND_LEN: usize = 8 * 1024;

/// Only one terminal command runs at a time. Several at once would multiply the
/// memory and processor cost on a device that has very little of either, and
/// there is no reason to run more than one root command concurrently.
static TERMINAL_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Run one command on the device and return what it printed.
///
/// Off unless `terminal_enabled` is set in the config, which the web interface
/// deliberately cannot do: it is set when flashing, with the installer's
/// `--enable-terminal` flag. The daemon runs as root, so this is the difference
/// between an interface that reads data and one that can do anything at all,
/// and turning it on should take physical access to the device.
///
/// Not a session. Each request is one command with no state carried between
/// them, so there is no shell to hijack and nothing to leave open.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/terminal",
    tag = "Debug",
    request_body(
        content = TerminalRequest,
        description = "The command to run. Runs as root in a fresh shell, with no state carried between requests."
    ),
    responses(
        (status = StatusCode::OK, description = "Command ran; see the body for its output", body = TerminalResponse),
        (status = StatusCode::FORBIDDEN, description = "Terminal not enabled on this device"),
        (status = StatusCode::BAD_REQUEST, description = "Empty command"),
    ),
    summary = "Run a command on the device",
))]
pub async fn run_terminal_command(
    State(state): State<Arc<ServerState>>,
    current: Option<axum::Extension<crate::web_auth::CurrentDevice>>,
    Json(body): Json<TerminalRequest>,
) -> Result<Json<TerminalResponse>, (StatusCode, String)> {
    if !state.config.terminal_enabled {
        return Err((
            StatusCode::FORBIDDEN,
            "the terminal is not enabled on this device. It can only be turned on when \
             flashing, with the installer's --enable-terminal flag."
                .to_string(),
        ));
    }
    // Defence in depth: the root shell must never be reachable without a web
    // password, even though the rest of the interface is open when none is set.
    // The global auth middleware passes everything through when no account
    // exists, so without this check an enabled terminal on an account-less
    // device would be unauthenticated root command execution. Requiring an
    // account here is independent of that middleware.
    // The second gate: a step-up confirmed on the unit itself, per browser,
    // and each command keeps it open a little longer. 428 tells the
    // interface to ask for one rather than showing a bare failure.
    let device_id = device_id_of(&current);
    if !state.stepup.active(&device_id) {
        return Err((
            StatusCode::PRECONDITION_REQUIRED,
            r#"{"stepup":"required"}"#.to_string(),
        ));
    }
    state.stepup.extend(&device_id);
    let command = body.command.trim();
    if command.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no command given".to_string()));
    }
    if command.len() > TERMINAL_MAX_COMMAND_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("command is too long (limit {TERMINAL_MAX_COMMAND_LEN} bytes)"),
        ));
    }

    // One command at a time. If another is already running, refuse rather than
    // pile a second root process onto the device.
    let Ok(_slot) = TERMINAL_SLOT.try_acquire() else {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "another terminal command is already running; try again once it finishes".to_string(),
        ));
    };

    // Scoped here: a different `AsyncReadExt` is in use elsewhere in this file.
    use tokio::io::AsyncReadExt as _;

    let spawned = tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Its own process group, so a command that starts other processes can
        // be stopped as a whole. Killing only the shell leaves whatever it
        // started running on a device with one slow core to spare.
        .process_group(0)
        // Covers the paths that return early without reaching the kill below.
        .kill_on_drop(true)
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            return Ok(Json(TerminalResponse {
                stdout: String::new(),
                stderr: format!("failed to run: {err}"),
                exit_code: None,
                timed_out: false,
            }));
        }
    };

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let mut out = Vec::new();
    let mut err = Vec::new();

    let finished = tokio::time::timeout(TERMINAL_TIMEOUT, async {
        // Both pipes have to be drained while the command runs, not after it
        // exits. A command writing more than the pipe buffer holds blocks until
        // something reads it, so waiting for exit first would hang forever on
        // exactly the noisy commands this is most useful for.
        // Read at most a little past the display limit from each pipe, not the
        // whole stream. read_to_end would buffer everything the command emits
        // in memory before anything trimmed it, so a command that prints
        // without end could exhaust the device's memory before the cap ever
        // applied. Once a pipe hits the cap we stop reading it; the command then
        // blocks on the full pipe and the timeout below kills it.
        let cap = TERMINAL_MAX_OUTPUT as u64 + 1;
        let read_out = async {
            if let Some(pipe) = stdout_pipe.as_mut() {
                let _ = pipe.take(cap).read_to_end(&mut out).await;
            }
        };
        let read_err = async {
            if let Some(pipe) = stderr_pipe.as_mut() {
                let _ = pipe.take(cap).read_to_end(&mut err).await;
            }
        };
        tokio::join!(read_out, read_err);
        child.wait().await
    })
    .await;

    let (exit_code, timed_out) = match finished {
        Ok(Ok(status)) => (status.code(), false),
        Ok(Err(wait_err)) => {
            return Ok(Json(TerminalResponse {
                stdout: String::new(),
                stderr: format!("failed to run: {wait_err}"),
                exit_code: None,
                timed_out: false,
            }));
        }
        Err(_) => {
            kill_process_group(&mut child).await;
            (None, true)
        }
    };

    // Whatever it managed to print is kept, including when it was killed. A
    // command that printed something useful and then hung should not come back
    // empty. `timed_out` is what says it was killed, so no message is invented
    // here: the caller would otherwise show the same thing twice.
    Ok(Json(TerminalResponse {
        stdout: truncate_output(String::from_utf8_lossy(&out).to_string()),
        stderr: truncate_output(String::from_utf8_lossy(&err).to_string()),
        exit_code,
        timed_out,
    }))
}

/// Kill a timed-out command and everything it started.
///
/// The child leads its own process group, so signalling the negative pid
/// reaches the group. Without this the command carried on running after being
/// reported as killed, which was both a lie and a way to load up a device that
/// has very little to spare.
async fn kill_process_group(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        // Negative pid means "the whole group", which is why the child was put
        // into one of its own.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    // Reap it, so it does not sit around as a zombie.
    let _ = child.wait().await;
}

/// Cap output, saying so rather than silently cutting it off.
fn truncate_output(mut text: String) -> String {
    if text.len() > TERMINAL_MAX_OUTPUT {
        // The limit may land inside a multi-byte character, and String::truncate
        // panics there. The firmware profile aborts on panic, so that would take
        // the whole daemon down mid-recording.
        let mut cut = TERMINAL_MAX_OUTPUT;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n[output truncated]");
    }
    text
}
