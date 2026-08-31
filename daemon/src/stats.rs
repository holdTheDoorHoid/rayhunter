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
}

impl SystemStats {
    pub async fn new(qmdl_path: &str, device: &Device) -> Result<Self, String> {
        Ok(Self {
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

/// Warmest processor sensor and warmest power amplifier sensor.
///
/// Sensor naming varies by platform, so this groups by name rather than
/// assuming an index: `pa_therm` is a power amplifier, and anything else that
/// reports a plausible temperature is treated as a processor sensor. Values are
/// millidegrees on some platforms and degrees on others, so anything above 200
/// is scaled down.
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
        let celsius = if value.abs() > 200.0 {
            value / 1000.0
        } else {
            value
        };
        // Discard obvious nonsense rather than reporting it.
        if !(-40.0..=150.0).contains(&celsius) {
            continue;
        }
        let name = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        let slot = if name.contains("pa_therm") {
            &mut radio
        } else {
            &mut cpu
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
    match SystemStats::new(qmdl_store.path.to_str().unwrap(), &state.config.device).await {
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
}
