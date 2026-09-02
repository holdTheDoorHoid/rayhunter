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
mod gps;
mod http_client;
mod key_input;
mod notifications;
mod packet_explorer;
mod pcap;
mod qmdl_store;
mod redact;
mod server;
mod sim_health;
mod stats;
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
    MAX_GIF_BYTES, ServerState, annotate_recording, debug_clear_qr, debug_keypress,
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

// Runs the axum server, taking all the elements needed to build up our
// ServerState and a oneshot Receiver that'll fire when it's time to shutdown
// (i.e. user hit ctrl+c)
async fn run_server(
    task_tracker: &TaskTracker,
    state: Arc<ServerState>,
    addrs: Vec<IpAddr>,
    shutdown_token: CancellationToken,
) -> JoinHandle<()> {
    info!("spinning up server");
    let port = state.config.port;
    let tls_port = state.config.tls_port;
    let tls_identity = state.tls.clone();
    // Wrapped around every route. When no accounts are configured the layer
    // passes everything through, so this changes nothing until somebody adds
    // one.
    let app = get_router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            web_auth::require_auth,
        ))
        // Outermost, so a cross-site state-changing request is refused before
        // anything else looks at it. Independent of whether a password is set.
        .layer(axum::middleware::from_fn(web_auth::csrf_protection))
        .with_state(state);

    info!("serving the web interface on port {port}, addresses {addrs:?}");

    // The same interface again, over TLS, on every address the plain one
    // uses. Alongside rather than instead: nothing about the plain port
    // changes until pairing is enforced, so a unit updated in the field
    // behaves exactly as before with one more port open. Any failure here
    // costs the TLS port and nothing else.
    if let Some(identity) = tls_identity {
        match tls::server_config(&identity) {
            Ok(config) => {
                for ip in &addrs {
                    let sock = SocketAddr::new(*ip, tls_port);
                    let listener = match tls::TlsListener::bind(sock, config.clone()).await {
                        Ok(listener) => listener,
                        Err(e) => {
                            error!("couldn't bind the TLS web interface to {sock}: {e}");
                            continue;
                        }
                    };
                    let app = app.clone();
                    let shutdown = shutdown_token.clone();
                    task_tracker.spawn(async move {
                        info!("The orca is hunting for stingrays... ({sock}, TLS)");
                        axum::serve(listener, app)
                            .with_graceful_shutdown(shutdown.cancelled_owned())
                            .await
                            .unwrap();
                    });
                }
                info!(
                    "serving the web interface over TLS on port {tls_port}, fingerprint {}",
                    identity.fingerprint_hex()
                );
            }
            Err(e) => error!("TLS is unavailable: {e}"),
        }
    }

    // One listener per address, all shut down together. The listeners bind the
    // hotspot and loopback but not the WAN side; see select_listen_addrs.
    let mut last_handle = None;
    for ip in addrs {
        let sock = SocketAddr::new(ip, port);
        let listener = match TcpListener::bind(&sock).await {
            Ok(listener) => listener,
            Err(e) => {
                error!("couldn't bind the web interface to {sock}: {e}");
                continue;
            }
        };
        let app = app.clone();
        let shutdown = shutdown_token.clone();
        last_handle = Some(task_tracker.spawn(async move {
            info!("The orca is hunting for stingrays... ({sock})");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap();
        }));
    }

    // If every chosen address failed to bind, serve on all interfaces so the
    // device is still reachable rather than silently offering no interface.
    match last_handle {
        Some(handle) => handle,
        None => {
            let sock = SocketAddr::from(([0, 0, 0, 0], port));
            let listener = TcpListener::bind(&sock).await.unwrap();
            task_tracker.spawn(async move {
                info!("The orca is hunting for stingrays... ({sock}, fallback)");
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown_token.cancelled_owned())
                    .await
                    .unwrap();
            })
        }
    }
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
        match tls::load_or_generate(Path::new(&config.auth_store_path), &hotspot_addrs).await {
            Ok(identity) => Some(Arc::new(identity)),
            Err(e) => {
                error!("TLS is unavailable this run: {e}");
                None
            }
        }
    };

    let state = Arc::new(ServerState {
        config_path: args.config_path.clone(),
        tls: tls_identity,
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
    run_server(&task_tracker, state, listen_addrs, shutdown_token.clone()).await;

    task_tracker.close();
    task_tracker.wait().await;

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
