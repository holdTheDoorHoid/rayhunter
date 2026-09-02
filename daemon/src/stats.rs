use std::ffi::CString;
use std::sync::Arc;

use crate::battery::get_battery_status;
use crate::error::RayhunterError;
use crate::server::ServerState;
use crate::update::UpdateStatus;
use crate::{battery::BatteryState, qmdl_store::ManifestEntry};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use log::error;
use rayhunter::{Device, util::RuntimeMetadata};
use serde::Serialize;
use tokio::process::Command;

/// Structure of device system statistics
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct SystemStats {
    pub disk_stats: DiskStats,
    pub memory_stats: MemoryStats,
    pub runtime_metadata: RuntimeMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_status: Option<BatteryState>,
    /// How hard the device is working, and how long it has been up. Absent on
    /// platforms that do not expose these, rather than reported as zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthStats>,
    /// Whether ADB is on, and whether it can be changed from here at all.
    ///
    /// Reported alongside the other read-only facts about the device rather
    /// than in the configuration, because it describes what the device is
    /// currently doing. The setting that asks for a change is separate, and
    /// only takes effect at the next restart.
    pub adb: crate::adb_control::AdbState,
    /// Where recordings are going, and how the memory card is doing.
    pub storage: crate::storage::StorageStatus,
}

impl SystemStats {
    pub async fn new(
        qmdl_path: &str,
        device: &Device,
        storage: crate::storage::StorageStatus,
    ) -> Result<Self, String> {
        Ok(Self {
            adb: crate::adb_control::current_state(),
            storage,
            disk_stats: DiskStats::new(qmdl_path)?,
            memory_stats: MemoryStats::new(device).await?,
            runtime_metadata: RuntimeMetadata::new(),
            health: HealthStats::read(),
            battery_status: match get_battery_status(device).await {
                Ok(status) => Some(status),
                Err(RayhunterError::FunctionNotSupportedForDeviceError) => None,
                Err(err) => {
                    log::error!("Failed to get battery status: {err}");
                    None
                }
            },
        })
    }
}

/// Load, uptime and temperature, read from the kernel.
///
/// These matter for a device that is meant to run unattended. A silent reboot
/// leaves a gap in coverage that nothing else would reveal, and a device that
/// cannot keep up may drop radio messages, which would make a missed detection
/// look like a quiet night.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct HealthStats {
    /// Seconds since the device booted.
    pub uptime_secs: u64,
    /// Load average over one, five and fifteen minutes.
    pub load_avg: [f32; 3],
    /// How many cores that load is spread across. One on these devices, which
    /// is why a load above 1 means work is queuing rather than merely busy.
    pub cpu_count: usize,
    /// Warmest processor sensor, in degrees Celsius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_temp_c: Option<f32>,
    /// Warmest power amplifier sensor. These track how hard the radio is
    /// transmitting rather than how busy the processor is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radio_temp_c: Option<f32>,
    /// Share of the processor actually in use, 0 to 100, measured between
    /// requests.
    ///
    /// This is the figure that answers "is the device keeping up". Load
    /// average does not: it counts tasks waiting on anything at all, so it can
    /// sit near 1 on a device that is 80% idle. Measured here rather than
    /// derived from load, because they are different quantities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_busy_percent: Option<f32>,
    /// Share of the processor this daemon is responsible for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rayhunter_cpu_percent: Option<f32>,
}

/// Previous processor counters, so usage can be measured across requests
/// rather than reported as a meaningless instantaneous value.
static LAST_CPU_SAMPLE: std::sync::Mutex<Option<CpuSample>> = std::sync::Mutex::new(None);

#[derive(Clone, Copy)]
struct CpuSample {
    total: u64,
    idle: u64,
    process: u64,
}

fn read_cpu_sample() -> Option<CpuSample> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|f| f.parse().ok())
        .collect();
    if fields.len() < 5 {
        return None;
    }
    let total: u64 = fields.iter().sum();
    let idle = fields[3];

    // utime + stime for this process, in the same clock ticks.
    let process = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            // The command name can contain spaces inside parentheses, so
            // fields are counted from after the closing one.
            let rest = s.rsplit_once(") ")?.1;
            let f: Vec<&str> = rest.split_whitespace().collect();
            Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
        })
        .unwrap_or(0);

    Some(CpuSample {
        total,
        idle,
        process,
    })
}

/// Processor usage since the previous call, as (whole system, this daemon).
fn measure_cpu() -> (Option<f32>, Option<f32>) {
    let Some(now) = read_cpu_sample() else {
        return (None, None);
    };
    let mut last = match LAST_CPU_SAMPLE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let previous = last.replace(now);

    let Some(previous) = previous else {
        // First call has nothing to compare against; a figure now would be
        // usage since boot, which is not what anyone reading this wants.
        return (None, None);
    };
    let elapsed = now.total.saturating_sub(previous.total);
    if elapsed == 0 {
        return (None, None);
    }
    let idle = now.idle.saturating_sub(previous.idle);
    let busy = elapsed.saturating_sub(idle) as f32 / elapsed as f32 * 100.0;
    let mine = now.process.saturating_sub(previous.process) as f32 / elapsed as f32 * 100.0;
    (Some(busy.clamp(0.0, 100.0)), Some(mine.clamp(0.0, 100.0)))
}

impl HealthStats {
    pub fn read() -> Option<Self> {
        let uptime_secs = std::fs::read_to_string("/proc/uptime")
            .ok()?
            .split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()? as u64;

        let loadavg = std::fs::read_to_string("/proc/loadavg").ok()?;
        let mut fields = loadavg.split_whitespace();
        let load_avg = [
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
        ];

        let cpu_count = std::fs::read_to_string("/proc/cpuinfo")
            .map(|s| s.lines().filter(|l| l.starts_with("processor")).count())
            .unwrap_or(0)
            .max(1);

        let (cpu_temp_c, radio_temp_c) = read_temperatures();
        let (cpu_busy_percent, rayhunter_cpu_percent) = measure_cpu();

        Some(Self {
            uptime_secs,
            load_avg,
            cpu_count,
            cpu_temp_c,
            radio_temp_c,
            cpu_busy_percent,
            rayhunter_cpu_percent,
        })
    }
}

/// Which reading a thermal zone contributes to, if any.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Sensor {
    Processor,
    Radio,
}

/// The value a Qualcomm tsens channel reports when nothing is wired to it.
///
/// A TP-Link M7350 v8.0 has two power amplifier channels, and only one is
/// populated: `pa_therm0` reads 29 while `pa_therm1` reads exactly 125 whatever
/// the device is doing. Taking the warmest of the two therefore reported a
/// permanent 125 C radio, which the interface printed as "125°C Radio".
///
/// Dropping this reading costs a genuine 125 C measurement. That is a trade
/// worth making: a power amplifier that really reached 125 C would have
/// triggered the firmware's own thermal shutdown long before, whereas the false
/// reading is displayed on every single device that has an unpopulated channel.
const TSENS_UNPOPULATED_C: f32 = 125.0;

/// Zones that are definitely not the processor, matched by name.
///
/// Everything else still falls through to the processor, because sensor naming
/// varies by platform and an allowlist would silently drop the reading on any
/// device whose cores are not called what this file expects. Only names with
/// clear evidence behind them belong here: an mdm9607 exposes a `battery` zone
/// reporting tenths of a degree, which the old "not a power amplifier, so a
/// processor" rule filed as a core and scaled from 2800 to 2.8 C.
const NOT_PROCESSOR: [&str; 3] = ["battery", "bms", "chg"];

/// Which reading a zone belongs to, given its `type` and raw `temp`.
///
/// Classified by name rather than by index, since the naming varies by
/// platform. Deliberately tolerant in the same direction as before: an
/// unrecognised zone is treated as a processor sensor rather than discarded, so
/// a device this file has never seen still reports something.
fn classify_thermal_zone(name: &str, raw: f32) -> Option<(Sensor, f32)> {
    // Millidegrees on some platforms, whole degrees on others.
    let celsius = if raw.abs() > 200.0 { raw / 1000.0 } else { raw };

    // Discard obvious nonsense rather than reporting it.
    if !(-40.0..=150.0).contains(&celsius) {
        return None;
    }

    if name.contains("pa_therm") {
        if celsius == TSENS_UNPOPULATED_C {
            return None;
        }
        return Some((Sensor::Radio, celsius));
    }

    if NOT_PROCESSOR.iter().any(|n| name.contains(n)) {
        return None;
    }

    Some((Sensor::Processor, celsius))
}

/// Warmest processor sensor and warmest power amplifier sensor.
fn read_temperatures() -> (Option<f32>, Option<f32>) {
    let Ok(entries) = std::fs::read_dir("/sys/class/thermal") else {
        return (None, None);
    };
    let mut cpu: Option<f32> = None;
    let mut radio: Option<f32> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(raw) = std::fs::read_to_string(path.join("temp")) else {
            continue;
        };
        let Ok(value) = raw.trim().parse::<f32>() else {
            continue;
        };
        let name = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        let Some((sensor, celsius)) = classify_thermal_zone(name.trim(), value) else {
            continue;
        };
        let slot = match sensor {
            Sensor::Radio => &mut radio,
            Sensor::Processor => &mut cpu,
        };
        *slot = Some(slot.map_or(celsius, |c: f32| c.max(celsius)));
    }
    (cpu, radio)
}

/// Device storage information
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct DiskStats {
    /// The partition to which the daemon is installed
    partition: String,
    /// The total disk size of the partition
    total_size: String,
    /// Total used size of the partition
    used_size: String,
    /// Remaining free space of the partition
    available_size: String,
    /// Disk usage displayed as percentage
    used_percent: String,
    /// The root folder to which the partition is mounted
    mounted_on: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

impl DiskStats {
    #[allow(clippy::unnecessary_cast)] // c_ulong is u32 on ARM, u64 on macOS
    pub fn new(qmdl_path: &str) -> Result<Self, String> {
        let c_path =
            CString::new(qmdl_path).map_err(|e| format!("invalid path {qmdl_path}: {e}"))?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return Err(format!(
                "statvfs({qmdl_path}) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let block_size = stat.f_frsize as u64;
        let total_kb = (stat.f_blocks as u64 * block_size / 1024) as usize;
        let free_kb = (stat.f_bfree as u64 * block_size / 1024) as usize;
        let available_kb = (stat.f_bavail as u64 * block_size / 1024) as usize;
        let used_kb = total_kb.saturating_sub(free_kb);
        let used_percent = format!(
            "{}%",
            ((stat.f_blocks - stat.f_bfree) * 100)
                .checked_div(stat.f_blocks)
                .unwrap_or(0)
        );

        Ok(Self {
            partition: qmdl_path.to_string(),
            total_size: humanize_kb(total_kb),
            used_size: humanize_kb(used_kb),
            available_size: humanize_kb(available_kb),
            used_percent,
            mounted_on: qmdl_path.to_string(),
            available_bytes: Some(stat.f_bavail as u64 * block_size),
            total_bytes: Some(stat.f_blocks as u64 * block_size),
        })
    }
}

/// Device memory information
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct MemoryStats {
    /// The total memory available on the device
    total: String,
    /// The currently used memory
    used: String,
    /// Remaining free memory
    free: String,
}

// runs the given command and returns its stdout as a string
async fn get_cmd_output(mut cmd: Command) -> Result<String, String> {
    let cmd_str = format!("{:?}", cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("error running command {}: {}", cmd_str, e))?;
    if !output.status.success() {
        // A command killed by a signal (the OOM killer, most likely here) has
        // no exit code, and unwrapping one aborts the whole daemon under the
        // firmware profile.
        let reason = match output.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "a signal".to_string(),
        };
        return Err(format!("command {cmd_str} failed with {reason}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

impl MemoryStats {
    // runs "free -k" and parses the output to retrieve memory stats for most devices,
    pub async fn new(device: &Device) -> Result<Self, String> {
        // Use busybox for Uz801
        let mut free_cmd: Command;
        if matches!(device, Device::Uz801) {
            free_cmd = Command::new("busybox");
            free_cmd.arg("free");
        } else {
            free_cmd = Command::new("free");
        }
        free_cmd.arg("-k");
        let stdout = get_cmd_output(free_cmd).await?;
        let mut numbers = stdout
            .split_whitespace()
            .flat_map(|part| part.parse::<usize>());
        Ok(Self {
            total: humanize_kb(numbers.next().ok_or("error parsing free output")?),
            used: humanize_kb(numbers.next().ok_or("error parsing free output")?),
            free: humanize_kb(numbers.next().ok_or("error parsing free output")?),
        })
    }
}

// turns a number of kilobytes (like 28293) into a human-readable string (like "28.3M")
fn humanize_kb(kb: usize) -> String {
    if kb < 1000 {
        return format!("{kb}K");
    }
    format!("{:.1}M", kb as f64 / 1024.0)
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/system-stats",
    tag = "Statistics",
    responses(
        (status = StatusCode::OK, description = "Success", body = SystemStats),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Error collecting statistics")
    ),
    summary = "Get system info",
    description = "Display system/device statistics."
))]
pub async fn get_system_stats(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<SystemStats>, (StatusCode, String)> {
    let qmdl_store = state.qmdl_store_lock.read().await;
    let storage = state.storage_status.read().await.clone();
    match SystemStats::new(
        qmdl_store.path.to_str().unwrap(),
        &state.config.device,
        storage,
    )
    .await
    {
        Ok(stats) => Ok(Json(stats)),
        Err(err) => {
            error!("error getting system stats: {err}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "error getting system stats".to_string(),
            ))
        }
    }
}

/// QMDL manifest information
#[derive(Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ManifestStats {
    /// A vector containing the names of the QMDL files
    pub entries: Vec<ManifestEntry>,
    /// The currently open QMDL file
    pub current_entry: Option<ManifestEntry>,
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/qmdl-manifest",
    tag = "Statistics",
    responses(
        (status = StatusCode::OK, description = "Success", body = ManifestStats)
    ),
    summary = "QMDL Manifest",
    description = "List QMDL files available on the device and some of their basic statistics."
))]
pub async fn get_qmdl_manifest(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<ManifestStats>, (StatusCode, String)> {
    let qmdl_store = state.qmdl_store_lock.read().await;
    let mut entries = qmdl_store.manifest.entries.clone();
    let current_entry = qmdl_store.current_entry.map(|index| entries.remove(index));
    Ok(Json(ManifestStats {
        entries,
        current_entry,
    }))
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/update-status",
    tag = "Statistics",
    responses(
        (status = StatusCode::OK, description = "Success", body = UpdateStatus)
    ),
    summary = "Rayhunter update status",
    description = "Check for available updates for Rayhunter."
))]
pub async fn get_update_status(State(state): State<Arc<ServerState>>) -> Json<UpdateStatus> {
    Json(state.update_status_lock.read().await.clone())
}

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/log",
    tag = "Statistics",
    responses(
        (status = StatusCode::OK, description = "Success", content_type = "text/plain"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Could not read /data/rayhunter/rayhunter.log file")
    ),
    summary = "Display log",
    description = "Download the current device log in UTF-8 plaintext."
))]
pub async fn get_log() -> Result<String, (StatusCode, String)> {
    tokio::fs::read_to_string("/data/rayhunter/rayhunter.log")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A helper killed by a signal has no exit code. That is exactly what the
    /// OOM killer does on these devices, and unwrapping the absent code aborts
    /// the daemon under the firmware profile.
    #[tokio::test]
    async fn a_command_killed_by_a_signal_is_an_error_not_a_panic() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "kill -KILL $$"]);
        let result = get_cmd_output(cmd).await;
        let err = result.expect_err("a killed command must be an error");
        assert!(err.contains("signal"), "unexpected message: {err}");
    }

    #[tokio::test]
    async fn a_failing_command_reports_its_exit_code() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exit 3"]);
        let err = get_cmd_output(cmd).await.expect_err("exit 3 is a failure");
        assert!(err.contains("exit code 3"), "unexpected message: {err}");
    }

    /// The zones of a TP-Link M7350 v8.0 (mdm9607), read off the device.
    const M7350_ZONES: [(&str, f32); 8] = [
        ("battery", 2800.0),
        ("tsens_tz_sensor0", 35.0),
        ("tsens_tz_sensor1", 35.0),
        ("tsens_tz_sensor2", 36.0),
        ("tsens_tz_sensor3", 35.0),
        ("tsens_tz_sensor4", 36.0),
        ("pa_therm0", 29.0),
        ("pa_therm1", 125.0),
    ];

    fn warmest(zones: &[(&str, f32)]) -> (Option<f32>, Option<f32>) {
        let mut cpu: Option<f32> = None;
        let mut radio: Option<f32> = None;
        for (name, raw) in zones {
            let Some((sensor, celsius)) = classify_thermal_zone(name, *raw) else {
                continue;
            };
            let slot = match sensor {
                Sensor::Radio => &mut radio,
                Sensor::Processor => &mut cpu,
            };
            *slot = Some(slot.map_or(celsius, |c: f32| c.max(celsius)));
        }
        (cpu, radio)
    }

    /// The bug this replaced: `pa_therm1` is not wired up on this board and
    /// reads a flat 125, so taking the warmest power amplifier reported a radio
    /// permanently on fire.
    #[test]
    fn an_unpopulated_power_amplifier_channel_does_not_become_the_radio_reading() {
        let (_, radio) = warmest(&M7350_ZONES);
        assert_eq!(
            radio,
            Some(29.0),
            "should report pa_therm0, not the 125 sentinel"
        );
    }

    /// The battery zone reports tenths of a degree and is not a processor.
    /// Counting it as one filed 28.0 C as 2.8 C worth of "processor".
    #[test]
    fn the_battery_zone_is_not_counted_as_a_processor() {
        assert_eq!(classify_thermal_zone("battery", 2800.0), None);
        let (cpu, _) = warmest(&M7350_ZONES);
        assert_eq!(cpu, Some(36.0), "should be the warmest tsens core");
    }

    /// A board where every power amplifier channel is unpopulated has no radio
    /// reading at all, which is honest. Reporting 125 was not.
    #[test]
    fn no_usable_power_amplifier_means_no_radio_reading() {
        let zones = [
            ("pa_therm0", 125.0),
            ("pa_therm1", 125.0),
            ("tsens_tz_sensor0", 40.0),
        ];
        let (cpu, radio) = warmest(&zones);
        assert_eq!(radio, None);
        assert_eq!(cpu, Some(40.0));
    }

    #[test]
    fn millidegrees_are_scaled_and_nonsense_is_dropped() {
        assert_eq!(
            classify_thermal_zone("tsens_tz_sensor0", 42000.0),
            Some((Sensor::Processor, 42.0))
        );
        // Below absolute plausibility for a device somebody is carrying.
        assert_eq!(classify_thermal_zone("tsens_tz_sensor0", -50.0), None);
        // Above it.
        assert_eq!(classify_thermal_zone("tsens_tz_sensor0", 200.0), None);
    }

    /// An unrecognised zone still counts as a processor. Narrowing this to an
    /// allowlist would silently drop the processor reading on every device
    /// whose cores are not named the way an mdm9607's are, which is the whole
    /// reason the original matched by name in the first place.
    #[test]
    fn an_unknown_zone_is_still_treated_as_a_processor() {
        assert_eq!(
            classify_thermal_zone("some-future-sensor", 45.0),
            Some((Sensor::Processor, 45.0))
        );
        // Names an Orbic or another Qualcomm board might plausibly use.
        for name in ["cpu-0-0-usr", "xo_therm", "quiet_therm", "mdm-core-usr"] {
            assert_eq!(
                classify_thermal_zone(name, 41.0),
                Some((Sensor::Processor, 41.0)),
                "{name} should still report"
            );
        }
    }
}
