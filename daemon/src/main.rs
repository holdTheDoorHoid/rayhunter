mod adb_control;
mod analysis;
mod battery;
mod cell_info;
mod cleanup;
mod config;
mod crypto_provider;
mod demo;
mod diag;
mod display;
mod error;
mod export_metadata;
mod frontdoor;
mod gps;
mod http_client;
mod key_input;
mod mdns;
mod notifications;
mod packet_explorer;
mod pairing;
mod pcap;
mod qmdl_store;
mod redact;
mod server;
mod sim_health;
mod stats;
mod stepup;
mod subscriber_id;
mod timing_advance;
mod tls;
mod update;
mod web_auth;
mod webdav;
mod wifi_ap;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use crate::battery::run_battery_notification_worker;
use crate::config::{GpsMode, parse_args, parse_config};
use crate::diag::run_diag_read_thread;
use crate::error::RayhunterError;
use crate::gps::{get_gps, post_gps};
use crate::notifications::{NotificationService, run_notification_worker};
use crate::packet_explorer::{get_packet, list_packets};
use crate::pcap::get_pcap;
use crate::qmdl_store::RecordingStore;
use crate::server::{
    MAX_GIF_BYTES, ServerState, annotate_recording, complete_setup, debug_clear_qr, debug_keypress,
    debug_set_display_state, debug_show_qr, delete_display_gif, delete_web_user, get_cell_info,
    get_config, get_display_gif, get_qmdl, get_time, get_tls_info, get_wifi_status, get_zip,
    run_terminal_command, scan_wifi, serve_static, set_config, set_display_gif, set_time_offset,
    set_web_user, test_notification, trigger_demo_warning,
};
use crate::stats::{get_qmdl_manifest, get_system_stats, get_update_status};
use crate::update::{UpdateStatus, run_update_check_worker};
use crate::webdav::run_webdav_upload_worker;
use wifi_station::WifiStatus;

use analysis::{
    AnalysisCtrlMessage, AnalysisStatus, get_analysis_status, run_analysis_thread, start_analysis,
};
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::response::Redirect;
use axum::routing::{get, post};
use diag::{
    DiagDeviceCtrlMessage, delete_all_recordings, delete_recording, get_analysis_report,
    start_recording, stop_recording,
};
use log::{error, info, warn};
use qmdl_store::RecordingStoreError;
use rayhunter::Device;
use stats::get_log;
use tokio::net::TcpListener;
use tokio::select;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{self, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

type AppRouter = Router<Arc<ServerState>>;

fn get_router() -> AppRouter {
    Router::new()
        .route("/api/pcap/{name}", get(get_pcap))
        .route("/api/qmdl/{name}", get(get_qmdl))
        .route("/api/zip/{name}", get(get_zip))
        .route("/api/system-stats", get(get_system_stats))
        .route("/api/update-status", get(get_update_status))
        .route("/api/qmdl-manifest", get(get_qmdl_manifest))
        .route("/api/log", get(get_log))
        .route("/api/start-recording", post(start_recording))
        .route("/api/stop-recording", post(stop_recording))
        .route("/api/delete-recording/{name}", post(delete_recording))
        .route("/api/annotate-recording/{name}", post(annotate_recording))
        .route("/api/delete-all-recordings", post(delete_all_recordings))
        .route("/api/analysis-report/{name}", get(get_analysis_report))
        .route("/api/analysis", get(get_analysis_status))
        .route("/api/analysis/{name}", post(start_analysis))
        .route("/api/cell-info", get(get_cell_info))
        .route("/api/packets/{recording}", get(list_packets))
        .route("/api/packets/{recording}/{packet_num}", get(get_packet))
        .route("/api/demo-warning", post(trigger_demo_warning))
        .route("/api/config", get(get_config))
        .route("/api/config", post(set_config))
        .route("/api/web-users", post(set_web_user))
        .route("/api/web-users/{username}/delete", post(delete_web_user))
        .route(
            "/api/display-gif/{state}",
            get(get_display_gif)
                .post(set_display_gif)
                .layer(DefaultBodyLimit::max(MAX_GIF_BYTES)),
        )
        .route("/api/display-gif/{state}/delete", post(delete_display_gif))
        .route("/api/test-notification", post(test_notification))
        .route("/api/wifi-status", get(get_wifi_status))
        .route("/api/wifi-scan", post(scan_wifi))
        .route("/api/time", get(get_time))
        .route("/api/time-offset", post(set_time_offset))
        .route("/api/tls-info", get(get_tls_info))
        .route("/api/ca.pem", get(server::get_ca_pem))
        .route("/api/ca.crt", get(server::get_ca_der))
        .route("/api/ca.mobileconfig", get(server::get_ca_mobileconfig))
        .route("/api/setup/status", get(server::get_setup_status))
        .route("/api/setup/complete", post(complete_setup))
        .route("/api/pair/passphrase", post(server::pair_with_passphrase))
        .route("/api/pair/account", post(server::pair_with_account))
        .route("/api/devices", get(server::list_devices))
        .route("/api/devices/{id}/rename", post(server::rename_device))
        .route("/api/devices/{id}/revoke", post(server::revoke_device))
        .route("/api/devices/code", post(server::mint_pair_code))
        .route("/api/pair/code", post(server::pair_with_code))
        .route("/api/passphrase", post(server::change_passphrase))
        .route("/api/setup/press-request", post(server::request_press))
        .route("/api/setup/press-status/{id}", get(server::press_status))
        .route(
            "/api/setup/complete-press",
            post(server::complete_setup_by_press),
        )
        .route("/api/stepup/start", post(server::stepup_start))
        .route("/api/stepup/confirm", post(server::stepup_confirm))
        .route("/api/stepup/status", get(server::stepup_status))
        .route("/api/stepup/end", post(server::stepup_end))
        .route("/p/{code}", get(server::serve_pair_page))
        .route("/P/{code}", get(server::serve_pair_page))
        .route("/pair", get(server::serve_pair_page))
        .route("/s/{token}", get(server::serve_pair_page))
        .route("/S/{token}", get(server::serve_pair_page))
        .route("/api/debug/display-state", post(debug_set_display_state))
        .route("/api/debug/keypress", post(debug_keypress))
        .route("/api/debug/qr", post(debug_show_qr))
        .route("/api/debug/qr/clear", post(debug_clear_qr))
        .route("/api/terminal", post(run_terminal_command))
        .route("/api/gps", get(get_gps))
        .route("/api/gps", post(post_gps))
        .route("/", get(|| async { Redirect::permanent("/index.html") }))
        .route("/{*path}", get(serve_static))
}

/// How long to wait for a private LAN interface to appear before serving on all
/// interfaces instead. The WiFi hotspot address can be configured slightly after
/// the daemon starts at boot, so a brief wait catches it; the fallback means a
/// device is never left unreachable over its own WiFi if it does not.
const WEB_BIND_INTERFACE_ATTEMPTS: u32 = 10;

/// Choose the addresses the web server binds to, given the machine's interface
/// IPs.
///
/// Loopback always — that is USB/adb access and the device talking to itself —
/// plus every RFC1918 private address, which is the WiFi hotspot the device
/// serves. The cellular/WAN interface, which carries a public or carrier-NAT
/// (CGNAT) address, is deliberately left off, so the interface is not reachable
/// from the internet side even on a device with a live SIM.
///
/// If no private address is present at all, it returns `0.0.0.0` (every
/// interface) as a fallback. A device that cannot be reached over its own WiFi
/// is far worse than the small exposure this removes, and this is defence in
/// depth, not a lock.
fn select_listen_addrs(interface_ips: &[IpAddr]) -> Vec<IpAddr> {
    let mut addrs = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    for ip in interface_ips {
        if let IpAddr::V4(v4) = ip
            && v4.is_private()
            && !addrs.contains(ip)
        {
            addrs.push(*ip);
        }
    }
    if addrs.len() == 1 {
        // Only loopback: no hotspot address found.
        return vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)];
    }
    addrs
}

/// The addresses to bind, waiting briefly for the hotspot interface to come up.
async fn web_listen_addrs() -> Vec<IpAddr> {
    for attempt in 0..WEB_BIND_INTERFACE_ATTEMPTS {
        let ips: Vec<IpAddr> = match if_addrs::get_if_addrs() {
            Ok(ifaces) => ifaces.iter().map(|i| i.ip()).collect(),
            Err(e) => {
                warn!("couldn't list network interfaces: {e}");
                Vec::new()
            }
        };
        let addrs = select_listen_addrs(&ips);
        // More than just loopback means a private (hotspot) address was found.
        if addrs.len() > 1 {
            return addrs;
        }
        if attempt + 1 < WEB_BIND_INTERFACE_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
    warn!("no private LAN interface found after waiting; serving on all interfaces");
    vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)]
}

/// Everything needed to serve the interface on one more address.
///
/// Built once, then used for every address the unit has at start and for
/// any that appears later, such as the STA address when the unit joins a
/// home network. Each address gets a plain listener and, when TLS is up, a
/// TLS one; each is stamped with what kind of listener it is.
struct Listeners {
    base: Router,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    port: u16,
    tls_port: u16,
}

impl Listeners {
    fn app_for(&self, kind: web_auth::ListenerKind) -> Router {
        self.base.clone().layer(axum::Extension(kind))
    }

    /// Bind `ip` and serve until `shutdown`. Returns whether the plain
    /// listener bound.
    async fn serve(
        &self,
        task_tracker: &TaskTracker,
        ip: IpAddr,
        shutdown: CancellationToken,
    ) -> bool {
        let loopback = ip.is_loopback();
        let sock = SocketAddr::new(ip, self.port);
        let mut bound = false;
        match TcpListener::bind(&sock).await {
            Ok(listener) => {
                let app = self.app_for(if loopback {
                    web_auth::ListenerKind::Loopback
                } else {
                    web_auth::ListenerKind::Plain
                });
                let shutdown = shutdown.clone();
                task_tracker.spawn(async move {
                    info!("The orca is hunting for stingrays... ({sock})");
                    axum::serve(listener, app)
                        .with_graceful_shutdown(shutdown.cancelled_owned())
                        .await
                        .unwrap();
                });
                bound = true;
            }
            Err(e) => error!("couldn't bind the web interface to {sock}: {e}"),
        }
        if let Some(config) = &self.tls_config {
            let sock = SocketAddr::new(ip, self.tls_port);
            match tls::TlsListener::bind(sock, config.clone()).await {
                Ok(listener) => {
                    let app = self.app_for(if loopback {
                        web_auth::ListenerKind::Loopback
                    } else {
                        web_auth::ListenerKind::Tls
                    });
                    task_tracker.spawn(async move {
                        info!("The orca is hunting for stingrays... ({sock}, TLS)");
                        axum::serve(listener, app)
                            .with_graceful_shutdown(shutdown.cancelled_owned())
                            .await
                            .unwrap();
                    });
                }
                Err(e) => error!("couldn't bind the TLS web interface to {sock}: {e}"),
            }
        }
        bound
    }
}

/// How often the STA address is looked at.
const STA_POLL: std::time::Duration = std::time::Duration::from_secs(3);

// Runs the axum server on every address the unit has, and on the STA
// address whenever it has one. Takes the ServerState and a token that fires
// when it is time to shut down (i.e. user hit ctrl+c).
async fn run_server(
    task_tracker: &TaskTracker,
    state: Arc<ServerState>,
    addrs: Vec<IpAddr>,
    mdns_addrs: mdns::SharedAddresses,
    shutdown_token: CancellationToken,
) {
    info!("spinning up server");
    let port = state.config.port;
    let tls_port = state.config.tls_port;
    // Wrapped around every route. Innermost decides who may pass; then a
    // cross-site state-changing request is refused before anything else
    // looks at it; outermost, the plain hotspot port is sent to TLS. Each
    // listener stamps the requests it accepts with what kind of listener it
    // is, which is what the redirect and the loopback exemption go by.
    let base = get_router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            web_auth::require_auth,
        ))
        .layer(axum::middleware::from_fn(web_auth::csrf_protection))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            web_auth::redirect_to_tls,
        ))
        .with_state(state.clone());

    // The same interface again, over TLS, on every address the plain one
    // uses. Alongside rather than instead, so a unit whose TLS fails keeps
    // its plain port; the redirect only happens while TLS is up.
    let tls_config =
        state
            .tls
            .as_ref()
            .and_then(|tls| match tls::server_config(tls.resolver.clone()) {
                Ok(config) => {
                    info!("serving the web interface over TLS on port {tls_port}");
                    Some(config)
                }
                Err(e) => {
                    error!("TLS is unavailable: {e}");
                    None
                }
            });
    let listeners = Arc::new(Listeners {
        base,
        tls_config,
        port,
        tls_port,
    });
    info!("serving the web interface on port {port}, addresses {addrs:?}");

    let mut any_bound = false;
    for ip in &addrs {
        any_bound |= listeners
            .serve(task_tracker, *ip, shutdown_token.clone())
            .await;
    }
    // If every chosen address failed to bind, serve on all interfaces so the
    // device is still reachable rather than silently offering no interface.
    if !any_bound {
        let sock = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(&sock).await.unwrap();
        let app = listeners.app_for(web_auth::ListenerKind::Plain);
        let shutdown = shutdown_token.clone();
        task_tracker.spawn(async move {
            info!("The orca is hunting for stingrays... ({sock}, fallback)");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap();
        });
    }

    // The STA address comes and goes with the network the unit joins, and
    // usually after the daemon has started, which used to mean it was never
    // served at all: reaching a unit on a home LAN was a race with its
    // startup. Watch for it, serve it while it lasts, and tell the mDNS
    // responder about it.
    let tracker = task_tracker.clone();
    let watcher_state = state.clone();
    task_tracker.spawn(async move {
        let mut served: Option<(std::net::Ipv4Addr, CancellationToken)> = None;
        loop {
            select! {
                _ = shutdown_token.cancelled() => break,
                _ = tokio::time::sleep(STA_POLL) => {}
            }
            let sta = watcher_state
                .wifi_status
                .read()
                .await
                .ip
                .as_deref()
                .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok());
            match (sta, &served) {
                (Some(ip), Some((current, _))) if *current == ip => {}
                (Some(ip), _) => {
                    if let Some((old, token)) = served.take() {
                        token.cancel();
                        mdns_addrs.write().await.retain(|a| *a != old);
                    }
                    if addrs.contains(&IpAddr::V4(ip)) {
                        // Already served since start.
                        continue;
                    }
                    info!("WiFi client address {ip} appeared; serving the web interface on it");
                    let token = shutdown_token.child_token();
                    listeners
                        .serve(&tracker, IpAddr::V4(ip), token.clone())
                        .await;
                    mdns_addrs.write().await.push(ip);
                    served = Some((ip, token));
                }
                (None, Some((old, _))) => {
                    info!("WiFi client address {old} is gone; no longer serving on it");
                    let (old, token) = served.take().unwrap();
                    token.cancel();
                    mdns_addrs.write().await.retain(|a| *a != old);
                }
                (None, None) => {}
            }
        }
    });
}

// Loads a RecordingStore if one exists, and if not, only create one if we're
// not in debug mode. If we fail to parse the manifest AND we're not in debug
// mode, try to recover the manifest from the existing QMDL files
async fn init_qmdl_store(config: &config::Config) -> Result<RecordingStore, RayhunterError> {
    let path = &config.qmdl_store_path;
    let dir_exists = tokio::fs::try_exists(path)
        .await
        .map_err(|e| RayhunterError::from(RecordingStoreError::OpenDirError(e)))?;
    let manifest_exists = dir_exists
        && tokio::fs::try_exists(std::path::Path::new(path).join("manifest.toml"))
            .await
            .map_err(|e| RayhunterError::from(RecordingStoreError::ReadManifestError(e)))?;

    if config.debug_mode {
        if manifest_exists {
            Ok(RecordingStore::load(path).await?)
        } else {
            Err(RayhunterError::NoStoreDebugMode(path.clone()))
        }
    } else if manifest_exists {
        match RecordingStore::load(path).await {
            Ok(store) => Ok(store),
            Err(RecordingStoreError::ParseManifestError(err)) => {
                error!("failed to parse QMDL manifest: {err}");
                info!("recovering manifest from existing QMDL files...");
                Ok(RecordingStore::recover(path).await?)
            }
            Err(err) => Err(err.into()),
        }
    } else if dir_exists {
        // The directory is there but the manifest is not. Reconstruct it from
        // the QMDL files on disk rather than starting fresh, which would leave
        // existing recordings physically present but invisible to Rayhunter.
        warn!(
            "recording directory exists but manifest.toml is missing; recovering from QMDL files"
        );
        Ok(RecordingStore::recover(path).await?)
    } else {
        Ok(RecordingStore::create(path).await?)
    }
}

// Start a thread that'll track when user hits ctrl+c. When that happens,
// trigger various cleanup tasks, including sending signals to other threads to
// shutdown
fn run_shutdown_thread(
    task_tracker: &TaskTracker,
    diag_device_sender: Sender<DiagDeviceCtrlMessage>,
    shutdown_token: CancellationToken,
    qmdl_store_lock: Arc<RwLock<RecordingStore>>,
    analysis_tx: Sender<AnalysisCtrlMessage>,
) -> JoinHandle<Result<(), RayhunterError>> {
    info!("create shutdown thread");

    task_tracker.spawn(async move {
        select! {
            res = tokio::signal::ctrl_c() => {
                if let Err(err) = res {
                    error!("Unable to listen for shutdown signal: {err}");
                }
            }
            _ = shutdown_token.cancelled() => {}
        }

        let mut qmdl_store = qmdl_store_lock.write().await;
        if qmdl_store.current_entry.is_some() {
            info!("Closing current QMDL entry...");
            qmdl_store.close_current_entry().await?;
            info!("Done!");
        }

        shutdown_token.cancel();
        diag_device_sender
            .send(DiagDeviceCtrlMessage::Exit)
            .await
            .expect("couldn't send Exit message to diag thread");
        analysis_tx
            .send(AnalysisCtrlMessage::Exit)
            .await
            .expect("couldn't send Exit message to analysis thread");
        Ok(())
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), RayhunterError> {
    rayhunter::init_logging(log::LevelFilter::Info);

    crate::crypto_provider::install_default();

    let args = parse_args();

    loop {
        let config = parse_config(&args.config_path).await?;
        if !run_with_config(&args, config).await? {
            return Ok(());
        }
    }
}

async fn run_with_config(
    args: &config::Args,
    config: config::Config,
) -> Result<bool, RayhunterError> {
    // TaskTrackers give us an interface to spawn tokio threads, and then
    // eventually await all of them ending
    let task_tracker = TaskTracker::new();
    println!("R A Y H U N T E R 🐳");

    let store = init_qmdl_store(&config).await?;
    let analysis_status = AnalysisStatus::new(&store);
    let qmdl_store_lock = Arc::new(RwLock::new(store));
    let (diag_tx, diag_rx) = mpsc::channel::<DiagDeviceCtrlMessage>(1);
    let (ui_update_tx, ui_update_rx) = mpsc::channel::<display::DisplayState>(1);
    // Shared between the display, the buttons and the server: a press asks the
    // display to step aside for a moment so the device's own screens can be
    // read. The server holds one so the behaviour can be triggered for testing,
    // since a physical button cannot be pressed from a script.
    let suppression: display::SharedSuppression =
        std::sync::Arc::new(display::DisplaySuppression::new());
    // A picture that takes the whole screen for a while: the pairing code.
    // Shared between the display, which paints it, and the server, which
    // puts it up.
    let display_override: display::SharedOverride =
        std::sync::Arc::new(display::DisplayOverride::new());
    let (analysis_tx, analysis_rx) = mpsc::channel::<AnalysisCtrlMessage>(5);
    let restart_token = CancellationToken::new();
    let shutdown_token = restart_token.child_token();
    // Ensure shutdown_token is cancelled when this function exits for any
    // reason (e.g. diag device init failure), so all spawned tasks get
    // signaled to stop.
    let _shutdown_guard = shutdown_token.clone().drop_guard();

    // Shared between the diag thread, which fills it, and the server, which
    // serves it to the web UI. Declared out here so the server still has one in
    // debug mode, where no diag thread runs and it simply stays empty.
    let cell_tracker = Arc::new(RwLock::new(cell_info::CellTracker::new()));

    let notification_service = NotificationService::new(config.ntfy_url.clone());
    let update_status_lock = Arc::new(RwLock::new(UpdateStatus::default()));

    // Where the web interface will listen decides what the certificate is
    // for, so the addresses are settled before the identity is loaded. The
    // identity is only ever made once there is a hotspot address to put in
    // it; a start that finds none waits for the next one rather than making
    // a certificate that names nothing.
    let listen_addrs = web_listen_addrs().await;
    let hotspot_addrs: Vec<IpAddr> = listen_addrs
        .iter()
        .copied()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .collect();
    // A second address of the unit's own on the hotspot, where
    // `rayhunter.local` is Rayhunter without a port. Opened before the
    // certificate is loaded, so the certificate names it too.
    let hotspot_v4 = hotspot_addrs.iter().find_map(|ip| match ip {
        IpAddr::V4(v4) => Some(*v4),
        IpAddr::V6(_) => None,
    });
    let front_door = match hotspot_v4 {
        Some(hotspot) if config.tls_port != 0 && !config.debug_mode => {
            let iface = if_addrs::get_if_addrs().ok().and_then(|ifs| {
                ifs.into_iter()
                    .find(|i| i.ip() == IpAddr::V4(hotspot))
                    .map(|i| i.name)
            });
            match iface {
                Some(iface) => {
                    frontdoor::FrontDoor::open(&iface, hotspot, config.port, config.tls_port).await
                }
                None => None,
            }
        }
        _ => None,
    };
    let mut cert_addrs = hotspot_addrs.clone();
    if let Some(door) = &front_door {
        cert_addrs.push(IpAddr::V4(door.alias));
    }

    let tls_identity = if config.tls_port == 0 {
        info!("TLS is switched off (tls_port = 0)");
        None
    } else if hotspot_addrs.is_empty()
        && !tokio::fs::try_exists(Path::new(&config.auth_store_path).join(tls::CERT_FILE))
            .await
            .unwrap_or(false)
    {
        warn!("no hotspot address yet and no certificate on file; TLS waits for the next start");
        None
    } else {
        let dir = Path::new(&config.auth_store_path);
        match tls::load_or_generate(dir, &cert_addrs).await {
            Ok(identity) => match tls::TlsRenewer::new(dir, cert_addrs.clone(), identity) {
                Ok(renewer) => Some(Arc::new(renewer)),
                Err(e) => {
                    error!("TLS is unavailable this run: {e}");
                    None
                }
            },
            Err(e) => {
                error!("TLS is unavailable this run: {e}");
                None
            }
        }
    };
    if let Some(tls) = &tls_identity {
        // The leaf is short-lived by design; look at it now and then so it
        // is replaced in good time, and again whenever the clock is set.
        let tls = tls.clone();
        let shutdown = shutdown_token.clone();
        task_tracker.spawn(async move {
            loop {
                select! {
                    _ = shutdown.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {}
                }
                tls.check().await;
            }
        });
    }

    // Who this unit trusts. The setup code goes on the screen when there is
    // one and there is a hotspot address for the link to point at.
    let setup_display = match (
        hotspot_addrs.first(),
        display::qr::screen_geometry(&config.device),
    ) {
        (Some(ip), Some(screen)) => Some(pairing::SetupDisplay {
            override_: display_override.clone(),
            screen,
            host: format!("{ip}:{}", config.tls_port),
        }),
        _ => None,
    };
    let pairing =
        match pairing::Pairing::load(Path::new(&config.auth_store_path), setup_display.clone())
            .await
        {
            Ok(pairing) => Arc::new(pairing),
            Err(e) => {
                // Not overwritten and not fatal: the unit runs, pairs only for
                // this session, and the file is left for somebody to look at.
                error!("the pairing store is unusable ({e}); pairing will not persist this run");
                Arc::new(pairing::Pairing::ephemeral(
                    pairing::AuthState::default(),
                    setup_display,
                ))
            }
        };
    // The terminal's second gate uses the same screen.
    let stepup = Arc::new(stepup::StepUp::new(
        display::qr::screen_geometry(&config.device).map(|screen| stepup::StepUpDisplay {
            override_: display_override.clone(),
            screen,
        }),
    ));
    if tls_identity.is_some()
        && !pairing.setup_complete().await
        && pairing.device_count().await == 0
    {
        match pairing.open_setup_window().await {
            Ok(token) => info!(
                "this unit has no owner yet; setup token {}",
                pairing::display_token(&token)
            ),
            Err(e) => warn!("could not open the setup window: {e}"),
        }
    } else if tls_identity.is_none() && !pairing.setup_complete().await {
        warn!(
            "this unit has no owner yet, but without TLS nobody can pair; the interface is open as before"
        );
    }

    if !config.debug_mode {
        // Reconcile ADB with what the settings ask for, before anything else
        // starts. It only takes effect at the next restart, since the USB
        // composition is chosen at boot, so doing it early costs nothing and
        // means a change made in the settings is already in place by the time
        // the device next comes up.
        if let Some(wanted) = config.adb_enabled {
            match adb_control::apply(wanted) {
                Ok(true) => info!(
                    "ADB will be {} after the next restart",
                    if wanted { "enabled" } else { "disabled" }
                ),
                Ok(false) => {}
                // Not an error worth stopping for: the device keeps whatever
                // ADB it already had, which is the safe outcome.
                Err(err) => warn!("leaving ADB as it is: {err}"),
            }
        }

        info!("Starting Diag Thread");
        let gps_fixed_coords = match (config.gps_fixed_latitude, config.gps_fixed_longitude) {
            (Some(lat), Some(lon)) => Some((lat, lon)),
            _ => None,
        };
        run_diag_read_thread(
            &task_tracker,
            config.device.clone(),
            config.diag_device_path().to_string(),
            diag_rx,
            diag_tx.clone(),
            ui_update_tx.clone(),
            qmdl_store_lock.clone(),
            analysis_tx.clone(),
            config.analyzers.clone(),
            notification_service.new_handler(),
            config.min_space_to_start_recording_mb,
            config.min_space_to_continue_recording_mb,
            config.max_recording_size_mb,
            config.max_recording_minutes,
            config.auto_delete_clean_recordings,
            !config.webdav.url.is_empty(),
            config.gps_mode,
            gps_fixed_coords,
            cell_tracker.clone(),
        );
        info!("Starting UI");

        let update_ui = match &config.device {
            Device::Orbic | Device::Moxee => display::orbic::update_ui,
            Device::Tplink => display::tplink::update_ui,
            Device::Tmobile => display::tmobile::update_ui,
            Device::Wingtech => display::wingtech::update_ui,
            Device::Pinephone => display::headless::update_ui,
            Device::Uz801 => display::uz801::update_ui,
        };
        update_ui(
            &task_tracker,
            &config,
            suppression.clone(),
            display_override.clone(),
            shutdown_token.clone(),
            ui_update_rx,
        );

        info!("Starting Key Input service");
        key_input::run_key_input_thread(
            &task_tracker,
            &config,
            diag_tx.clone(),
            suppression.clone(),
            Some(pairing.clone()),
            Some(stepup.clone()),
            shutdown_token.clone(),
        );

        if config.auto_check_updates {
            run_update_check_worker(
                &task_tracker,
                shutdown_token.clone(),
                update_status_lock.clone(),
                notification_service.new_handler(),
                config.enabled_notifications.clone(),
            );
        }
    }

    let analysis_status_lock = Arc::new(RwLock::new(analysis_status));
    run_analysis_thread(
        &task_tracker,
        analysis_rx,
        qmdl_store_lock.clone(),
        analysis_status_lock.clone(),
        config.analyzers.clone(),
        config.device.clone(),
        config.debug_mode,
    );

    run_shutdown_thread(
        &task_tracker,
        diag_tx.clone(),
        shutdown_token.clone(),
        qmdl_store_lock.clone(),
        analysis_tx.clone(),
    );

    if !config.debug_mode {
        run_battery_notification_worker(
            &task_tracker,
            config.device.clone(),
            notification_service.new_handler(),
            shutdown_token.clone(),
        );
    }

    run_notification_worker(
        &task_tracker,
        notification_service,
        config.enabled_notifications.clone(),
    );

    let wifi_status = Arc::new(RwLock::new(WifiStatus::default()));
    if !config.debug_mode {
        wifi_station::run_wifi_client(
            &task_tracker,
            &config.wifi_config(),
            shutdown_token.clone(),
            wifi_status.clone(),
        );
    }

    if !config.webdav.url.trim().is_empty() {
        run_webdav_upload_worker(
            &task_tracker,
            shutdown_token.clone(),
            qmdl_store_lock.clone(),
            config.webdav.clone().into(),
        );
    }
    let initial_gps = if config.gps_mode == GpsMode::Fixed {
        match (config.gps_fixed_latitude, config.gps_fixed_longitude) {
            (Some(lat), Some(lon)) => Some(gps::GpsData {
                latitude: lat,
                longitude: lon,
            }),
            _ => {
                warn!(
                    "gps_mode is Fixed but gps_fixed_latitude or gps_fixed_longitude is missing from config — no GPS coordinates will be recorded"
                );
                None
            }
        }
    } else {
        None
    };

    // The addresses `rayhunter.local` answers with: the hotspot's now, the
    // STA one when it appears.
    let mdns_addrs: mdns::SharedAddresses = Arc::new(RwLock::new(
        hotspot_addrs
            .iter()
            .filter_map(|ip| match ip {
                IpAddr::V4(v4) => Some(*v4),
                IpAddr::V6(_) => None,
            })
            .map(|v4| match &front_door {
                // The name points at Rayhunter's own address, not the
                // hotspot's, so it works without a port.
                Some(door) if Some(v4) == hotspot_v4 => door.alias,
                _ => v4,
            })
            .collect(),
    ));
    if !config.debug_mode {
        mdns::run(&task_tracker, mdns_addrs.clone(), shutdown_token.clone());
    }

    let state = Arc::new(ServerState {
        config_path: args.config_path.clone(),
        tls: tls_identity,
        pairing,
        stepup,
        web_users: Arc::new(tokio::sync::RwLock::new(config.web_users.clone())),
        config,
        qmdl_store_lock: qmdl_store_lock.clone(),
        diag_device_ctrl_sender: diag_tx,
        analysis_status_lock,
        analysis_sender: analysis_tx,
        daemon_restart_token: restart_token.clone(),
        ui_update_sender: Some(ui_update_tx),
        suppression: Some(suppression),
        display_override: Some(display_override),
        cell_tracker: cell_tracker.clone(),
        wifi_status,
        wifi_scan_lock: tokio::sync::Mutex::new(()),
        gps_state: Arc::new(tokio::sync::RwLock::new(initial_gps)),
        update_status_lock: update_status_lock.clone(),
    });
    run_server(
        &task_tracker,
        state,
        listen_addrs,
        mdns_addrs,
        shutdown_token.clone(),
    )
    .await;

    task_tracker.close();
    task_tracker.wait().await;
    if let Some(door) = &front_door {
        door.close().await;
    }

    info!("see you space cowboy...");
    Ok(restart_token.is_cancelled())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_router() {
        // assert that creating the router does not panic from invalid route patterns.
        let _ = get_router();
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    /// The hotspot (a private address) and loopback are bound; the cellular WAN,
    /// whether a public address or a carrier-NAT (CGNAT, 100.64/10) one, is not.
    #[test]
    fn listens_on_the_hotspot_and_loopback_but_not_the_wan() {
        let interfaces = [
            ip("127.0.0.1"),   // loopback
            ip("192.168.1.1"), // the WiFi hotspot
            ip("100.64.0.5"),  // cellular WAN behind carrier NAT
            ip("8.8.8.8"),     // a public address, for good measure
        ];
        let addrs = select_listen_addrs(&interfaces);
        assert!(addrs.contains(&ip("127.0.0.1")), "loopback must be bound");
        assert!(addrs.contains(&ip("192.168.1.1")), "hotspot must be bound");
        assert!(
            !addrs.contains(&ip("100.64.0.5")),
            "CGNAT WAN must not be bound"
        );
        assert!(
            !addrs.contains(&ip("8.8.8.8")),
            "public WAN must not be bound"
        );
    }

    /// All three RFC1918 ranges count as a hotspot, since supported devices use
    /// different LAN subnets.
    #[test]
    fn all_private_ranges_are_treated_as_the_hotspot() {
        for hotspot in ["10.0.0.1", "172.16.5.1", "192.168.8.1"] {
            let addrs = select_listen_addrs(&[ip(hotspot)]);
            assert!(addrs.contains(&ip(hotspot)), "{hotspot} should be bound");
            assert!(addrs.contains(&ip("127.0.0.1")));
        }
    }

    /// With no private interface — an unrecognised device, or the hotspot not up
    /// yet — fall back to every interface so the UI is never unreachable.
    #[test]
    fn falls_back_to_all_interfaces_when_no_private_address_exists() {
        assert_eq!(
            select_listen_addrs(&[ip("8.8.8.8")]),
            vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)]
        );
        assert_eq!(
            select_listen_addrs(&[]),
            vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)]
        );
    }
}
