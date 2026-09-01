//! One-shot Wi-Fi surveillance scan, for hardware bring-up and verification.
//!
//! This is not the radio daemon. It exists to prove the capture → parse →
//! match → evidence path end to end on real hardware, and to be the thing a
//! hardware-test procedure runs. The daemon that owns the interface, paces
//! scans, and bounds its own memory comes next.
//!
//! It must be run from a context with `CAP_NET_ADMIN` — on the Orbic RC400L
//! that means from init or via `AT+SYSCMD`, not from an adb shell, whose
//! capability bounding set is only `CAP_SETUID|CAP_SETGID`.
//!
//! Usage:
//!   rayhunter-radio-probe --iface <name> [--base <name>] [--out <path>]
//!   rayhunter-radio-probe --from-file <scan output> [--out <path>]

use rayhunter_radio::evidence::{DeviceRef, EvidenceRecord, RetentionPolicy, to_ndjson};
use rayhunter_radio::observation::{ObservationSource, RadioTech};
use rayhunter_radio::scan::parse_iw_scan;
use rayhunter_radio::signature::SignatureDb;
use std::process::{Command, ExitCode};

const BUILTIN_PACK: &str = include_str!("../../signatures/builtin-surveillance-signatures.json");

/// Interface the probe creates for scanning, kept distinct from the `wlan1`
/// that wifi-station uses for client mode so the two never fight over it.
const DEFAULT_SCAN_IFACE: &str = "rhscan0";
const DEFAULT_BASE_IFACE: &str = "wlan0";

struct Args {
    iface: String,
    base: String,
    from_file: Option<String>,
    out: Option<String>,
    keep_iface: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        iface: DEFAULT_SCAN_IFACE.to_string(),
        base: DEFAULT_BASE_IFACE.to_string(),
        from_file: None,
        out: None,
        keep_iface: false,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        match flag.as_str() {
            "--iface" => args.iface = argv.next().unwrap_or_default(),
            "--base" => args.base = argv.next().unwrap_or_default(),
            "--from-file" => args.from_file = argv.next(),
            "--out" => args.out = argv.next(),
            "--keep-iface" => args.keep_iface = true,
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }
    args
}

fn run(cmd: &str, cmd_args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(cmd_args)
        .output()
        .map_err(|e| format!("could not run {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} {}: {}",
            cmd_args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Add a managed virtual interface alongside the running access point.
///
/// The QCA9377 advertises `#{managed} <= 2, #{AP} <= 2, total <= 4` as a valid
/// interface combination, which is what makes scanning possible without taking
/// the hotspot down.
fn create_scan_iface(base: &str, iface: &str) -> Result<(), String> {
    if std::path::Path::new(&format!("/sys/class/net/{iface}")).exists() {
        return Ok(());
    }
    run(
        "iw",
        &["dev", base, "interface", "add", iface, "type", "managed"],
    )?;
    run("ip", &["link", "set", iface, "up"])?;
    Ok(())
}

fn main() -> ExitCode {
    let args = parse_args();

    let scan_text = match &args.from_file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("could not read {path}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            if let Err(e) = create_scan_iface(&args.base, &args.iface) {
                eprintln!("could not prepare scan interface: {e}");
                eprintln!(
                    "note: this needs CAP_NET_ADMIN; an adb shell on the RC400L does not have it"
                );
                return ExitCode::FAILURE;
            }
            match run("iw", &["dev", &args.iface, "scan"]) {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("scan failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };

    let db = match SignatureDb::from_json(BUILTIN_PACK) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("builtin signature pack is unusable: {e}");
            return ExitCode::FAILURE;
        }
    };
    let enabled = db.signatures.iter().filter(|s| s.enabled).count();

    let observations = parse_iw_scan(&scan_text);
    let now = chrono::Utc::now();
    // Per-run salt for pseudonymising devices that matched nothing. Derived
    // from the clock rather than a stored value so it cannot be used to
    // correlate across runs.
    let salt = now.timestamp_nanos_opt().unwrap_or(0).to_le_bytes();

    let mut records = Vec::new();
    let mut matched = 0usize;
    for obs in &observations {
        let payload = rayhunter_radio::ObservationPayload::Wifi(obs.clone());
        let detections = db.match_observation(&payload);
        let Some(addr) = obs.primary_address() else {
            continue;
        };
        let policy = if detections.is_empty() {
            RetentionPolicy::Pseudonymise
        } else {
            matched += 1;
            RetentionPolicy::Identify
        };
        let device = DeviceRef::new(addr, policy, &salt);
        // One record per detection, plus a bare record when nothing fired, so
        // the sidecar also carries the denominator a persistence score needs.
        if detections.is_empty() {
            records.push(record(&device, obs, None, now, &db));
        } else {
            for d in detections {
                records.push(record(&device, obs, Some(d), now, &db));
            }
        }
    }

    let ndjson = to_ndjson(&records);
    match &args.out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &ndjson) {
                eprintln!("could not write {path}: {e}");
                return ExitCode::FAILURE;
            }
            eprintln!("wrote {} records to {path}", records.len());
        }
        None => print!("{ndjson}"),
    }

    eprintln!(
        "scanned via {}: {} networks, {} enabled signatures, {} matches",
        args.from_file.as_deref().unwrap_or(&args.iface),
        observations.len(),
        enabled,
        matched
    );

    if args.from_file.is_none() && !args.keep_iface {
        let _ = run("iw", &["dev", &args.iface, "del"]);
    }
    ExitCode::SUCCESS
}

fn record(
    device: &DeviceRef,
    obs: &rayhunter_radio::WifiObservation,
    detection: Option<rayhunter_radio::Detection>,
    now: chrono::DateTime<chrono::Utc>,
    db: &SignatureDb,
) -> EvidenceRecord {
    EvidenceRecord {
        timestamp: now,
        recording_id: "probe".to_string(),
        technology: RadioTech::Wifi,
        source: ObservationSource::HostWifiScan,
        device: device.clone(),
        ssid: obs.ssid.as_ref().map(|s| s.display()),
        rssi_dbm: obs.rssi_dbm,
        frequency_mhz: obs.frequency_mhz,
        channel: obs.channel(),
        first_seen: now,
        last_seen: now,
        observation_count: 1,
        detection,
        rule_version: db.pack_version.clone(),
    }
}
