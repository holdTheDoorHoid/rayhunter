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
    // Older kernels name the machine just "Snapdragon"; cpuinfo's Hardware
    // line then says which one.
    let cpuinfo_soc = fs::read_to_string("/proc/cpuinfo")
        .await
        .ok()
        .and_then(|cpuinfo| cpuinfo_hardware(&cpuinfo));
    info.soc = pick_soc(read_trimmed("/sys/devices/soc0/machine").await, cpuinfo_soc);
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
            // Revisions whose UPnP daemon says nothing (v3) still answer
            // the vendor's own status call on the loopback.
            if info.hardware_version.is_none()
                && let Some((model, version)) = tplink_web_identity().await
            {
                info.model = model.or(info.model);
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

/// Whether a later look might learn more: the TP-Link's revision comes
/// from a web server that is not up yet when the daemon starts.
pub fn incomplete(info: &HardwareInfo, device: &Device) -> bool {
    matches!(device, Device::Tplink) && info.hardware_version.is_none()
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

/// The chipset name: the SoC's machine name when it says something, else
/// cpuinfo's Hardware line.
fn pick_soc(machine: Option<String>, cpuinfo: Option<String>) -> Option<String> {
    match machine {
        Some(machine) if machine.chars().any(|c| c.is_ascii_digit()) => Some(machine),
        Some(machine) => cpuinfo.or(Some(machine)),
        None => cpuinfo,
    }
}

/// Ask the TP-Link's own web interface what it is. The status call needs
/// no login and answers on every revision seen so far, but not always on
/// the loopback: the v3.0's server listens only on the LAN address, so
/// every address the device has is tried, loopback first.
async fn tplink_web_identity() -> Option<(Option<String>, Option<String>)> {
    let client = crate::http_client::client().ok()?;
    let own: Vec<(String, std::net::IpAddr)> = if_addrs::get_if_addrs()
        .map(|ifs| ifs.into_iter().map(|i| (i.name.clone(), i.ip())).collect())
        .unwrap_or_default();
    for address in status_call_addresses(&own) {
        let response = client
            .post(format!("http://{address}/cgi-bin/qcmap_web_cgi"))
            .timeout(std::time::Duration::from_secs(3))
            // What a browser's form post carries; the CGI rejects
            // application/json outright.
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(r#"{"module":"status","action":0}"#)
            .send()
            .await;
        let Ok(response) = response else {
            continue;
        };
        let Ok(body) = response.text().await else {
            continue;
        };
        if let Some(hardware) = tplink_status_hardware_ver(&body) {
            return Some(split_tplink_hardware_ver(&hardware));
        }
    }
    None
}

/// Loopback first, then the device's own private IPv4 addresses, without
/// repeats. The carrier-side interfaces are left out: an address there is
/// private too, but nothing answers on it and each try costs the timeout.
fn status_call_addresses(own: &[(String, std::net::IpAddr)]) -> Vec<String> {
    let mut list = vec!["127.0.0.1".to_string()];
    for (name, ip) in own {
        let carrier = ["rmnet", "wwan", "ppp", "qmi"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        if !carrier
            && let std::net::IpAddr::V4(v4) = ip
            && !v4.is_loopback()
            && v4.is_private()
        {
            let text = v4.to_string();
            if !list.contains(&text) {
                list.push(text);
            }
        }
    }
    list
}

/// `deviceInfo.hardwareVer` out of the status reply, without trusting the
/// rest of the document to be well formed.
fn tplink_status_hardware_ver(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("deviceInfo")?
        .get("hardwareVer")?
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `"M7350(EU) v3.0"` is a model, a region in parentheses, and a revision.
fn split_tplink_hardware_ver(text: &str) -> (Option<String>, Option<String>) {
    let text = text.trim();
    let (model_part, version) = match text.rsplit_once(' ') {
        Some((model, version)) if version.starts_with(['v', 'V']) => {
            (model, Some(version.to_string()))
        }
        _ => (text, None),
    };
    let model = model_part
        .split('(')
        .next()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string);
    (model, version)
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
    fn a_generic_machine_name_defers_to_cpuinfo() {
        let mdm = Some("MDM9207".to_string());
        let cpu = Some("Qualcomm MSM 9625 (Flattened Device Tree)".to_string());
        assert_eq!(pick_soc(mdm.clone(), cpu.clone()), mdm);
        assert_eq!(pick_soc(Some("Snapdragon".into()), cpu.clone()), cpu);
        assert_eq!(
            pick_soc(Some("Snapdragon".into()), None).as_deref(),
            Some("Snapdragon")
        );
        assert_eq!(pick_soc(None, cpu.clone()), cpu);
    }

    #[test]
    fn the_vendor_status_reply_names_model_and_revision() {
        let body = r#"{"factoryDefault":false,"deviceInfo":{"productID":"73501003","model":"M7350","hardwareVer":"M7350(EU) v3.0","firmwareVer":"1.1.1 Build 160330 Rel.1002n"},"battery":{"connected":true}}"#;
        let hardware = tplink_status_hardware_ver(body).unwrap();
        assert_eq!(hardware, "M7350(EU) v3.0");
        assert_eq!(
            split_tplink_hardware_ver(&hardware),
            (Some("M7350".into()), Some("v3.0".into()))
        );
        assert_eq!(
            split_tplink_hardware_ver("M7350 v8.0"),
            (Some("M7350".into()), Some("v8.0".into()))
        );
        assert_eq!(
            split_tplink_hardware_ver("M7350"),
            (Some("M7350".into()), None)
        );
        assert_eq!(tplink_status_hardware_ver("not json"), None);
        assert_eq!(tplink_status_hardware_ver(r#"{"deviceInfo":{}}"#), None);
    }

    #[test]
    fn status_call_tries_loopback_then_private_addresses_once_each() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        let own = [
            ("lo".to_string(), IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ("br0".to_string(), IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
            ("br0".to_string(), IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
            (
                "bridge0".to_string(),
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            ),
            (
                "rmnet_data0".to_string(),
                IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40)),
            ),
            ("wlan0".to_string(), IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
            ("lo".to_string(), IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ];
        assert_eq!(
            status_call_addresses(&own),
            vec!["127.0.0.1", "192.168.0.1", "10.0.0.5"]
        );
        assert_eq!(status_call_addresses(&[]), vec!["127.0.0.1"]);
    }

    #[test]
    fn device_names_match_the_config_spelling() {
        assert_eq!(device_name(&Device::Orbic), "orbic");
        assert_eq!(device_name(&Device::Tplink), "tplink");
    }
}
