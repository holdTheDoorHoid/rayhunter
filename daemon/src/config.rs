use log::warn;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use rayhunter::Device;
use rayhunter::analysis::analyzer::AnalyzerConfig;

use crate::error::RayhunterError;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum GpsMode {
    Disabled = 0,
    Fixed = 1,
    Api = 2,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum UiLevel {
    Invisible = 0,
    Subtle = 1,
    Demo = 2,
    EffLogo = 3,
    HighVisibility = 4,
    /// Play a user-uploaded GIF per display state. See [`DisplayGifs`].
    CustomGif = 5,
    TransFlag = 128,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum KeyInputMode {
    Disabled = 0,
    DoubleTapPower = 1,
}
use crate::notifications::NotificationType;

/// User-supplied overrides for the colors drawn on the device's own display.
///
/// Each field is an `#rrggbb` hex string. A field left as `None` keeps
/// Rayhunter's built-in color for that state, including the green-to-blue
/// substitution performed by `colorblind_mode`. Colors only apply to devices
/// with a color-capable display; one-bit displays (e.g. the TP-Link M7350)
/// draw status icons instead and ignore these values.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct DisplayColors {
    /// Color drawn while recording is paused (built-in default: white)
    pub paused: Option<String>,
    /// Color drawn while recording, and for informational events
    /// (built-in default: green, or blue when colorblind_mode is enabled)
    pub recording: Option<String>,
    /// Color drawn for low-severity warnings (built-in default: yellow)
    pub warning_low: Option<String>,
    /// Color drawn for medium-severity warnings (built-in default: orange)
    pub warning_medium: Option<String>,
    /// Color drawn for high-severity warnings (built-in default: red)
    pub warning_high: Option<String>,
}

/// The display states that can carry a user-chosen color or GIF. Kept as one
/// list so the colors and the GIFs stay in step with each other.
pub const DISPLAY_STATE_KEYS: [&str; 5] = [
    "paused",
    "recording",
    "warning_low",
    "warning_medium",
    "warning_high",
];

/// User-uploaded GIFs to play per display state, used when `ui_level` is
/// [`UiLevel::CustomGif`].
///
/// Each field holds the *original* filename of the upload, kept only so the web
/// UI can show what was uploaded. The GIF data itself lives in
/// `gif_store_path` under a name derived from the state, so a state with a
/// value here always has exactly one file on disk. A state left as `None` falls
/// back to drawing that state's colored status line instead.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct DisplayGifs {
    pub paused: Option<String>,
    pub recording: Option<String>,
    pub warning_low: Option<String>,
    pub warning_medium: Option<String>,
    pub warning_high: Option<String>,
}

impl DisplayGifs {
    /// The stored filename for `state`, if one has been uploaded.
    pub fn get(&self, state: &str) -> Option<&String> {
        match state {
            "paused" => self.paused.as_ref(),
            "recording" => self.recording.as_ref(),
            "warning_low" => self.warning_low.as_ref(),
            "warning_medium" => self.warning_medium.as_ref(),
            "warning_high" => self.warning_high.as_ref(),
            _ => None,
        }
    }
}

/// Parse an `#rrggbb` (or `rrggbb`) hex color into its red, green and blue
/// components. Returns `None` for any string that isn't exactly six hex digits,
/// so that a malformed value falls back to the built-in color rather than
/// preventing the display from drawing at all.
pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let component = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    Some((component(0..2)?, component(2..4)?, component(4..6)?))
}

/// The structure of a valid rayhunter configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct Config {
    /// Path to store QMDL files
    pub qmdl_store_path: String,
    /// Listening port
    pub port: u16,
    /// Debug mode
    pub debug_mode: bool,
    /// Internal device name
    pub device: Device,
    /// UI level
    pub ui_level: UiLevel,
    /// Colorblind mode
    pub colorblind_mode: bool,
    /// Per-state color overrides for the device display
    pub display_colors: DisplayColors,
    /// Height in pixels of the colored status line. `None` keeps the built-in
    /// 2px. Clamped to the display's height at draw time, so a value copied
    /// between devices with different screens can't produce a broken display.
    /// Ignored by `UiLevel::HighVisibility`, which always fills the screen.
    pub status_bar_height: Option<u32>,
    /// Per-state user-uploaded GIFs, used when ui_level is CustomGif
    pub display_gifs: DisplayGifs,
    /// Directory holding the GIFs named by `display_gifs`
    pub gif_store_path: String,
    /// Key input mode
    pub key_input_mode: KeyInputMode,
    /// ntfy.sh URL
    pub ntfy_url: Option<String>,
    /// Vector containing the types of enabled notifications
    pub enabled_notifications: Vec<NotificationType>,
    /// Whether Rayhunter should periodically check GitHub for new releases
    pub auto_check_updates: bool,
    /// Vector containing the list of enabled analyzers
    pub analyzers: AnalyzerConfig,
    /// Minimum disk space required to start a recording
    pub min_space_to_start_recording_mb: u64,
    /// Minimum disk space required to continue a recording
    pub min_space_to_continue_recording_mb: u64,
    /// GPS mode
    pub gps_mode: GpsMode,
    /// Fixed latitude used when gps_mode=1
    pub gps_fixed_latitude: Option<f64>,
    /// Fixed longitude used when gps_mode=1
    pub gps_fixed_longitude: Option<f64>,
    /// Wifi client SSID
    pub wifi_ssid: Option<String>,
    /// Wifi client password
    pub wifi_password: Option<String>,
    /// Wifi security type (wpa_psk or sae)
    pub wifi_security: Option<wifi_station::SecurityType>,
    /// Wifi client mode
    pub wifi_enabled: bool,
    /// Vector containing wifi client DNS servers
    pub dns_servers: Option<Vec<String>>,
    /// WebDAV upload configuration. The upload worker runs whenever `webdav.url` is non-empty.
    pub webdav: WebdavConfig,
}

/// Configuration for uploading finished QMDL recordings to a WebDAV server.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct WebdavConfig {
    /// WebDAV server base URL, e.g. "https://example.com/remote.php/files/untitaker/my-subfolder/"
    pub url: String,
    /// Optional username for HTTP Basic auth
    pub username: Option<String>,
    /// Optional password for HTTP Basic auth
    pub password: Option<String>,
    /// Timeout (in seconds) for each upload request
    pub upload_timeout_secs: u64,
    /// How often (in seconds) the worker scans for entries to upload
    pub poll_interval_secs: u64,
    /// Minimum age (in seconds) an entry must have before it becomes eligible for upload
    pub min_age_secs: i64,
    /// Delete the file locally after a successful upload
    pub delete_on_upload: bool,
}

impl Default for WebdavConfig {
    fn default() -> Self {
        WebdavConfig {
            url: String::new(),
            username: None,
            password: None,
            upload_timeout_secs: 300,
            poll_interval_secs: 3600,
            min_age_secs: 86400,
            delete_on_upload: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            qmdl_store_path: "/data/rayhunter/qmdl".to_string(),
            port: 8080,
            debug_mode: false,
            device: Device::Orbic,
            ui_level: UiLevel::Subtle,
            colorblind_mode: false,
            display_colors: DisplayColors::default(),
            status_bar_height: None,
            display_gifs: DisplayGifs::default(),
            gif_store_path: "/data/rayhunter/gifs".to_string(),
            key_input_mode: KeyInputMode::Disabled,
            analyzers: AnalyzerConfig::default(),
            ntfy_url: None,
            enabled_notifications: vec![NotificationType::Warning, NotificationType::LowBattery],
            auto_check_updates: true,
            min_space_to_start_recording_mb: 1,
            min_space_to_continue_recording_mb: 1,
            gps_mode: GpsMode::Disabled,
            gps_fixed_latitude: None,
            gps_fixed_longitude: None,
            wifi_ssid: None,
            wifi_password: None,
            wifi_security: None,
            wifi_enabled: false,
            dns_servers: None,
            webdav: WebdavConfig::default(),
        }
    }
}

impl Config {
    pub fn wifi_config(&self) -> wifi_station::WifiConfig {
        let (wpa_bin, hostapd_conf, ctrl_interface) = match self.device {
            Device::Tmobile | Device::Wingtech => (
                Some("/usr/sbin/wpa_supplicant".into()),
                Some("/data/configs/hostapd.conf".into()),
                None,
            ),
            Device::Uz801 => (
                Some("/system/bin/wpa_supplicant".into()),
                Some("/data/misc/wifi/hostapd.conf".into()),
                Some("/data/misc/wifi/sockets".into()),
            ),
            _ => (None, None, None),
        };
        wifi_station::WifiConfig {
            wifi_enabled: self.wifi_enabled,
            dns_servers: self.dns_servers.clone(),
            wifi_ssid: self.wifi_ssid.clone(),
            wifi_password: self.wifi_password.clone(),
            security_type: self.wifi_security,
            wpa_supplicant_bin: wpa_bin.or_else(|| resolve_bin("wpa_supplicant")),
            hostapd_conf,
            ctrl_interface,
            udhcpc_hook_path: Some("/data/rayhunter/udhcpc-hook.sh".into()),
            dhcp_lease_path: Some("/data/rayhunter/dhcp_lease".into()),
            wpa_conf_path: Some("/data/rayhunter/wpa_sta.conf".into()),
            iw_bin: resolve_bin("iw"),
            udhcpc_bin: resolve_bin("udhcpc"),
            crash_log_dir: Some("/data/rayhunter/crash-logs".into()),
            wakelock_name: Some("rayhunter".into()),
        }
    }
}

fn resolve_bin(name: &str) -> Option<String> {
    let local = format!("/data/rayhunter/bin/{name}");
    if std::path::Path::new(&local).exists() {
        return Some(local);
    }
    None
}

pub async fn parse_config<P>(path: P) -> Result<Config, RayhunterError>
where
    P: AsRef<std::path::Path>,
{
    let mut config = if let Ok(config_file) = tokio::fs::read_to_string(&path).await {
        toml::from_str(&config_file).map_err(RayhunterError::ConfigFileParsingError)?
    } else {
        warn!("unable to read config file, using default config");
        Config::default()
    };

    if let Some((ssid, security)) =
        wifi_station::read_network_from_wpa_conf("/data/rayhunter/wpa_sta.conf")
    {
        config.wifi_ssid = Some(ssid);
        config.wifi_security = Some(security);
    } else {
        config.wifi_ssid = None;
        config.wifi_security = None;
    }
    config.wifi_password = None;

    Ok(config)
}

pub struct Args {
    pub config_path: String,
}

pub fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        println!("Usage: {} /path/to/config/file", args[0]);
        std::process::exit(1);
    }
    Args {
        config_path: args[1].clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_hex_color;

    #[test]
    fn parses_hex_colors_with_and_without_leading_hash() {
        assert_eq!(parse_hex_color("#ff0000"), Some((0xff, 0, 0)));
        assert_eq!(parse_hex_color("00ff00"), Some((0, 0xff, 0)));
        assert_eq!(parse_hex_color("#0000FF"), Some((0, 0, 0xff)));
        assert_eq!(parse_hex_color("#ffa500"), Some((0xff, 0xa5, 0)));
    }

    #[test]
    fn rejects_malformed_hex_colors() {
        // Wrong length: shorthand and overlong forms are not supported.
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("#ff00000"), None);
        assert_eq!(parse_hex_color(""), None);
        // Non-hex characters, including a stray inner '#'.
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color("#ff 000"), None);
        assert_eq!(parse_hex_color("##ff000"), None);
    }
}
