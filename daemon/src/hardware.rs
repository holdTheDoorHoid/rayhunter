//! What hardware this is, read from the running system for the recording
//! sidecar.
//!
//! Best effort throughout. The hotspots expose different things: every
//! Qualcomm board has a device-tree model string and a chipset name, the
//! TP-Link states its hardware revision only on the command line of its UPnP
//! daemon, and the Wingtech keeps a version file. A field the firmware does
//! not expose is left empty rather than guessed.

use std::path::Path;

use log::debug;
use rayhunter::Device;
use rayhunter::recording_metadata::HardwareInfo;
use tokio::fs;

/// Read everything the running system says about the hardware.
pub async fn detect(device: &Device) -> HardwareInfo {
    let mut info = HardwareInfo {
        device: device_name(device),
        ..Default::default()
    };
    info.model = read_trimmed("/proc/device-tree/model").await;
    info.soc = match read_trimmed("/sys/devices/soc0/machine").await {
        Some(machine) => Some(machine),
        None => fs::read_to_string("/proc/cpuinfo")
            .await
            .ok()
            .and_then(|cpuinfo| cpuinfo_hardware(&cpuinfo)),
    };
    match device {
        Device::Tplink => {
            if let Some((model, version)) = tplink_upnpd_identity(Path::new("/proc")).await {
                // The vendor's own model name is more specific than the
                // device tree's generic board name.
                if model.is_some() {
                    info.model = model;
                }
                info.hardware_version = version;
            }
        }
        Device::Wingtech => {
            if let Some(contents) = read_trimmed("/etc/wt_version").await {
                let (hardware, software) = wingtech_versions(&contents);
                info.hardware_version = hardware;
                info.firmware_build = software;
            }
        }
        _ => {}
    }
    if info.firmware_build.is_none() {
        info.firmware_build = read_trimmed("/etc/version").await;
    }
    debug!("hardware: {info:?}");
    info
}

/// Rayhunter's name for the device type, as written in the config file.
fn device_name(device: &Device) -> String {
    format!("{device:?}").to_lowercase()
}

async fn read_trimmed(path: &str) -> Option<String> {
    let contents = fs::read(path).await.ok()?;
    let text = String::from_utf8_lossy(&contents);
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!text.is_empty()).then(|| text.to_string())
}

/// The `Hardware` line of `/proc/cpuinfo`, which ARM kernels use for the
/// board or chipset name.
fn cpuinfo_hardware(cpuinfo: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "Hardware").then(|| value.trim().to_string())
    })
}

/// The TP-Link's UPnP daemon is started as `upnpd -mn M7350 -mv v8.0 ...`,
/// which is the only place the firmware states its hardware revision.
async fn tplink_upnpd_identity(proc: &Path) -> Option<(Option<String>, Option<String>)> {
    let mut entries = fs::read_dir(proc).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")).await else {
            continue;
        };
        let args: Vec<String> = cmdline
            .split(|&b| b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        if let Some(identity) = upnpd_identity(&args) {
            return Some(identity);
        }
    }
    None
}

/// Pick the model (`-mn`) and hardware version (`-mv`) out of a command
/// line, when it is `upnpd`'s.
fn upnpd_identity(args: &[String]) -> Option<(Option<String>, Option<String>)> {
    let program = args.first()?;
    if program.rsplit('/').next() != Some("upnpd") {
        return None;
    }
    let value_after = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|i| args.get(i + 1))
            .filter(|value| !value.starts_with('-'))
            .cloned()
    };
    Some((value_after("-mn"), value_after("-mv")))
}

/// The Wingtech's `/etc/wt_version` is `KEY=value` lines. The keys naming
/// the hardware and software versions are picked out by name.
fn wingtech_versions(contents: &str) -> (Option<String>, Option<String>) {
    let mut hardware = None;
    let mut software = None;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_uppercase();
        let value = value.trim().trim_matches('"');
        if value.is_empty() {
            continue;
        }
        if hardware.is_none() && (key.contains("HW") || key.contains("HARDWARE")) {
            hardware = Some(value.to_string());
        } else if software.is_none()
            && (key.contains("SW") || key.contains("SOFTWARE") || key.contains("INNER"))
        {
            software = Some(value.to_string());
        }
    }
    (hardware, software)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpuinfo_hardware_line_is_found() {
        let cpuinfo = "processor\t: 0\nmodel name\t: ARMv7 Processor rev 5 (v7l)\n\
                       Hardware\t: Qualcomm Technologies, Inc MDM9207\nRevision\t: 0000\n";
        assert_eq!(
            cpuinfo_hardware(cpuinfo).as_deref(),
            Some("Qualcomm Technologies, Inc MDM9207")
        );
        assert_eq!(cpuinfo_hardware("processor: 0\n"), None);
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upnpd_command_line_gives_model_and_revision() {
        let identity = upnpd_identity(&args(&["/usr/sbin/upnpd", "-mn", "M7350", "-mv", "v8.0"]));
        assert_eq!(identity, Some((Some("M7350".into()), Some("v8.0".into()))));
        // Another program with the same flags is not consulted.
        assert_eq!(upnpd_identity(&args(&["sh", "-mn", "M7350"])), None);
        // A flag with no value gives nothing for that field.
        assert_eq!(
            upnpd_identity(&args(&["upnpd", "-mn", "-mv", "v8.0"])),
            Some((None, Some("v8.0".into())))
        );
        assert_eq!(upnpd_identity(&[]), None);
    }

    #[test]
    fn wingtech_version_file_is_read_by_key_name() {
        let contents = "WT_HARDWARE_VERSION=\"HW1.2\"\nWT_SOFTWARE_VERSION=\"SW3.4\"\nOTHER=x\n";
        assert_eq!(
            wingtech_versions(contents),
            (Some("HW1.2".into()), Some("SW3.4".into()))
        );
        assert_eq!(wingtech_versions("nothing here\n"), (None, None));
    }

    #[test]
    fn device_names_match_the_config_spelling() {
        assert_eq!(device_name(&Device::Orbic), "orbic");
        assert_eq!(device_name(&Device::Tplink), "tplink");
    }
}
