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

/// How Rayhunter should keep its clock in sync when it drifts from a known-good
/// time source. Currently the only source is the browser talking to the web UI;
/// additional sources (e.g. network/NTP, GPS) may be added later.
///
/// Serialized as an integer: 0 = off (never warn or sync), 1 = autosync (silently
/// correct the drift), 2 = prompt (warn and let the user choose).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum ClockSyncMode {
    /// Never warn about or correct clock drift.
    Off = 0,
    /// Silently sync the device clock to the time source when drift is detected.
    Autosync = 1,
    /// Show a warning and let the user choose whether to sync.
    Prompt = 2,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum KeyInputMode {
    Disabled = 0,
    DoubleTapPower = 1,
}

/// Whether to stop the device's screen blanking on its own timer.
///
/// Three states rather than two booleans, because "do not keep the screen on,
/// but only while plugged in" is not a thing anybody can mean, and a pair of
/// flags would let a config express it.
///
/// `WhenPluggedIn` exists because an always-on backlight is the fastest way to
/// flatten one of these batteries, which is the objection that closed the
/// upstream attempt at this (EFForg/rayhunter#919). Left on a desk with power,
/// which is what people actually asked for in #539, the cost is nothing.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum KeepScreenOn {
    /// Let the device blank the screen as it normally would.
    Never = 0,
    /// Hold the screen on whatever the power source.
    Always = 1,
    /// Hold the screen on only while external power is connected.
    WhenPluggedIn = 2,
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
    /// Where the modem's diagnostic character device lives.
    ///
    /// `None` uses `/dev/diag`, which is right on every device Rayhunter
    /// currently supports. It is configurable because on hardware where the
    /// modem sits behind MHI or PCIe the node is somewhere else entirely, and
    /// that is the only thing standing between those devices and working:
    /// see EFForg/rayhunter#649.
    pub diag_device_path: Option<String>,
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
    /// Enables the demo control in the web UI, which injects a synthetic
    /// warning for showing Rayhunter to an audience. Off unless deliberately
    /// switched on, since it writes clearly labelled fake data into a recording.
    pub demo_mode: bool,
    /// Key input mode
    pub key_input_mode: KeyInputMode,
    /// Accounts permitted to use the web interface.
    ///
    /// Empty means no authentication, which is how Rayhunter has always
    /// behaved and stays the default so an update never locks anyone out of
    /// their own device. Adding an account turns authentication on.
    ///
    /// There is no TLS on these devices, so this is a second factor beyond
    /// knowing the WiFi password rather than protection against someone able
    /// to capture the traffic itself.
    pub web_users: Vec<crate::web_auth::WebUser>,
    /// Whether the web interface may run commands on the device.
    ///
    /// Deliberately not settable from the web interface: it can only be turned
    /// on when flashing, with the installer's --enable-terminal flag. The
    /// daemon runs as root, so this is the difference between an interface that
    /// reads data and one that can do anything at all. Requiring physical
    /// access to enable it means a mistake in the web interface cannot.
    pub terminal_enabled: bool,
    /// Whether to disclose this device's own IMSI, IMEI and temporary identity
    /// over the web API.
    ///
    /// Off by default, and deliberately so. The web interface has no
    /// authentication: anyone who can reach it, which on these devices means
    /// anyone on the hotspot's WiFi, can read anything it serves. An IMSI is
    /// the identifier an IMSI catcher exists to collect, so a detector that
    /// hands it out unasked would be working against its own purpose.
    pub show_subscriber_identity: bool,
    /// Whether to stop the screen blanking on the device's own timer.
    ///
    /// Device specific. Implemented for the Orbic; other devices ignore it
    /// rather than failing, so the setting is safe to carry in any config.
    pub keep_screen_on: KeepScreenOn,
    /// Shrink to the thin status line for a moment after a button is pressed,
    /// so the device's own screens can be read.
    ///
    /// Only affects the display levels that cover the screen. In those, the
    /// manufacturer's interface is completely hidden, wifi password included,
    /// and somebody who cannot read it has locked themselves out of their own
    /// hotspot. A button press is how a person navigates that interface, so it
    /// is a good signal that they want to see it.
    pub pause_display_on_keypress: bool,
    /// Whether ADB should be on after the next restart.
    ///
    /// `None`, the default, leaves the device exactly as its installer left
    /// it: devices that already have ADB keep it. Only devices whose USB
    /// composition is chosen by a value this recognises can be changed at all;
    /// see `crate::adb_control`.
    #[serde(default)]
    pub adb_enabled: Option<bool>,
    /// Let a burst of button presses switch the device's own WiFi access
    /// point off.
    ///
    /// Off by default. The access point is how most people reach the web
    /// interface, so this is not something to have switched on without meaning
    /// it. Restarting the device always brings the access point back, whatever
    /// this is set to, which is what stops it locking anybody out.
    #[serde(default)]
    pub wifi_ap_button_toggle: bool,
    /// How many presses the gesture takes.
    ///
    /// Enough that a device in a bag cannot produce it. Clamped when used, so
    /// a value typed in here cannot make the gesture trivially easy.
    #[serde(default = "default_wifi_ap_toggle_presses")]
    pub wifi_ap_toggle_presses: u8,
    /// The seconds all those presses have to land within.
    #[serde(default = "default_wifi_ap_toggle_window_secs")]
    pub wifi_ap_toggle_window_secs: u64,
    /// Whether the access point comes back on its own or waits for a restart.
    #[serde(default)]
    pub wifi_ap_off_mode: crate::wifi_ap::WifiApOffMode,
    /// Minutes before the access point comes back, in the temporary mode.
    #[serde(default = "default_wifi_ap_off_minutes")]
    pub wifi_ap_off_minutes: u64,
    /// ntfy.sh URL
    pub ntfy_url: Option<String>,
    /// Vector containing the types of enabled notifications
    pub enabled_notifications: Vec<NotificationType>,
    /// Whether Rayhunter should periodically check GitHub for new releases
    pub auto_check_updates: bool,
    /// How Rayhunter should handle its clock drifting from a known-good time source
    pub clock_sync_mode: ClockSyncMode,
    /// Vector containing the list of enabled analyzers
    pub analyzers: AnalyzerConfig,
    /// Minimum disk space required to start a recording
    pub min_space_to_start_recording_mb: u64,
    /// Minimum disk space required to continue a recording
    pub min_space_to_continue_recording_mb: u64,
    /// Delete recordings that found nothing, oldest first, when space runs low.
    ///
    /// Only ever removes recordings that have been analysed and raised no
    /// warning, are not the one being written, and are not still waiting to be
    /// uploaded. Anything not understood is kept, since not knowing what is in
    /// a recording is not a reason to delete it.
    pub auto_delete_clean_recordings: bool,
    /// Close the current recording and open a new one once it reaches this
    /// many megabytes. `None` leaves a recording running until it is stopped.
    ///
    /// Splitting a capture into pieces keeps any single file small enough to
    /// download over the device's own wifi, and means a recording is analysed
    /// and readable while capture continues rather than only at the end.
    pub max_recording_size_mb: Option<u64>,
    /// Close the current recording and open a new one once it has been running
    /// this many minutes. `None` disables rotation on time.
    ///
    /// Set alongside `max_recording_size_mb`, whichever comes first wins.
    pub max_recording_minutes: Option<u64>,
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

impl WebdavConfig {
    /// Reject durations that cannot be represented or would wedge the worker,
    /// before the config is persisted. Without this, a value near the i64
    /// extremes panics the duration constructor at startup — turning one bad
    /// config request into a daemon that crashes on every boot until the file
    /// is repaired by hand. A zero poll interval would busy-loop the worker.
    ///
    /// One week of polling, one hour per request, and ten years of minimum age
    /// are all far past anything sensible while staying well inside the ranges
    /// the runtime can represent.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=3600).contains(&self.upload_timeout_secs) {
            return Err(format!(
                "webdav upload_timeout_secs must be between 1 and 3600, got {}",
                self.upload_timeout_secs
            ));
        }
        if !(1..=604_800).contains(&self.poll_interval_secs) {
            return Err(format!(
                "webdav poll_interval_secs must be between 1 and 604800, got {}",
                self.poll_interval_secs
            ));
        }
        if !(0..=315_360_000).contains(&self.min_age_secs) {
            return Err(format!(
                "webdav min_age_secs must be between 0 and 315360000, got {}",
                self.min_age_secs
            ));
        }
        Ok(())
    }
}

fn default_wifi_ap_toggle_presses() -> u8 {
    5
}

fn default_wifi_ap_toggle_window_secs() -> u64 {
    4
}

fn default_wifi_ap_off_minutes() -> u64 {
    60
}

impl Default for Config {
    fn default() -> Self {
        Config {
            qmdl_store_path: "/data/rayhunter/qmdl".to_string(),
            diag_device_path: None,
            port: 8080,
            debug_mode: false,
            device: Device::Orbic,
            ui_level: UiLevel::Subtle,
            colorblind_mode: false,
            demo_mode: false,
            display_colors: DisplayColors::default(),
            status_bar_height: None,
            display_gifs: DisplayGifs::default(),
            gif_store_path: "/data/rayhunter/gifs".to_string(),
            key_input_mode: KeyInputMode::Disabled,
            show_subscriber_identity: false,
            web_users: Vec::new(),
            terminal_enabled: false,
            // Off by default. It holds a backlight on, which is a real cost
            // to a device that may be running on battery.
            keep_screen_on: KeepScreenOn::Never,
            // On by default. It costs a thin status line for twenty seconds
            // and it prevents somebody being locked out of their own hotspot.
            pause_display_on_keypress: true,
            // None by default: leave ADB exactly as the installer left it.
            adb_enabled: None,
            // Off by default: switching off the access point is switching off
            // the way most people reach this device, which nobody should get
            // by accident.
            wifi_ap_button_toggle: false,
            // Five presses inside four seconds. Deliberate to do, and not
            // something a pocket or a bag produces.
            wifi_ap_toggle_presses: 5,
            wifi_ap_toggle_window_secs: 4,
            wifi_ap_off_mode: crate::wifi_ap::WifiApOffMode::Temporary,
            // An hour. Long enough to be worth doing, short enough that
            // forgetting about it is not a problem.
            wifi_ap_off_minutes: 60,
            analyzers: AnalyzerConfig::default(),
            ntfy_url: None,
            enabled_notifications: vec![NotificationType::Warning, NotificationType::LowBattery],
            auto_check_updates: true,
            clock_sync_mode: ClockSyncMode::Prompt,
            min_space_to_start_recording_mb: 1,
            min_space_to_continue_recording_mb: 1,
            // Off by default. Deleting somebody's captures without being asked
            // is not something to do quietly, however good the reason.
            auto_delete_clean_recordings: false,
            // Off by default: rotation changes how recordings are grouped, and
            // a device that silently split its capture into pieces nobody asked
            // for would be surprising.
            max_recording_size_mb: None,
            max_recording_minutes: None,
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
    /// Where to open the modem's diagnostic device.
    ///
    /// Falls back to the usual node when unset, so an existing config keeps
    /// working and nobody has to set this to get the supported devices going.
    pub fn diag_device_path(&self) -> &str {
        self.diag_device_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(rayhunter::diag_device::DEFAULT_DIAG_PATH)
    }

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
    use super::Config;
    use super::WebdavConfig;
    use super::parse_hex_color;

    /// The defaults must pass validation, and the extreme values that used to
    /// panic the duration constructor at startup must be rejected up front.
    #[test]
    fn webdav_config_validation_rejects_out_of_range_durations() {
        assert!(WebdavConfig::default().validate().is_ok());

        let bad_min_age = WebdavConfig {
            min_age_secs: i64::MAX,
            ..WebdavConfig::default()
        };
        assert!(
            bad_min_age.validate().is_err(),
            "i64::MAX min age must be rejected"
        );

        let negative_min_age = WebdavConfig {
            min_age_secs: -1,
            ..WebdavConfig::default()
        };
        assert!(
            negative_min_age.validate().is_err(),
            "negative min age must be rejected"
        );

        let zero_poll = WebdavConfig {
            poll_interval_secs: 0,
            ..WebdavConfig::default()
        };
        assert!(
            zero_poll.validate().is_err(),
            "zero poll interval must be rejected"
        );

        let zero_timeout = WebdavConfig {
            upload_timeout_secs: 0,
            ..WebdavConfig::default()
        };
        assert!(
            zero_timeout.validate().is_err(),
            "zero timeout must be rejected"
        );
    }

    /// The i64::MAX value the API now rejects must, if it somehow reaches the
    /// runtime anyway, be handled without panicking (try_seconds, not seconds).
    #[test]
    fn an_extreme_min_age_does_not_panic_the_worker_config() {
        let cfg = WebdavConfig {
            url: "https://example.com/".to_string(),
            min_age_secs: i64::MAX,
            ..WebdavConfig::default()
        };
        // Would panic with TimeDelta::seconds; must not with try_seconds.
        let _ = crate::webdav::WebdavUploadWorkerConfig::from(cfg);
    }

    /// A config written before a detector existed must come up with that
    /// detector on, not silently off. This is the upgrade path for every
    /// device in the field: their `config.toml` lists only the analyzers of
    /// the version that wrote it.
    #[test]
    fn analyzers_missing_from_an_old_config_default_on() {
        let old_config = r#"
            ui_level = 1

            [analyzers]
            imsi_requested = true
            null_cipher = false
        "#;
        let config: super::Config = toml::from_str(old_config).unwrap();
        // The setting the old file does carry is respected...
        assert!(!config.analyzers.null_cipher);
        // ...and ones it predates come up enabled, matching the defaults.
        assert!(config.analyzers.lpp_location_request);
        assert!(config.analyzers.lpp_location_tracking);
        assert!(config.analyzers.rrlp_location_request);
        assert!(config.analyzers.incomplete_sib);
        // The testing analyzer stays off by default even when unlisted.
        assert!(!config.analyzers.test_analyzer);
    }

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

    /// An absent setting has to keep the supported devices working, since
    /// nobody should have to configure this to get an Orbic going.
    #[test]
    fn an_unset_diag_path_falls_back_to_the_usual_node() {
        let config = Config::default();
        assert_eq!(config.diag_device_path(), "/dev/diag");
    }

    #[test]
    fn a_configured_diag_path_is_used() {
        let config = Config {
            diag_device_path: Some("/dev/mhi_DIAG".to_string()),
            ..Config::default()
        };
        assert_eq!(config.diag_device_path(), "/dev/mhi_DIAG");
    }

    /// A key left blank in a hand edited config means "not set", not "open the
    /// empty path", which would fail with an error nobody could act on.
    #[test]
    fn a_blank_diag_path_falls_back_rather_than_failing() {
        for blank in ["", "   "] {
            let config = Config {
                diag_device_path: Some(blank.to_string()),
                ..Config::default()
            };
            assert_eq!(config.diag_device_path(), "/dev/diag", "for {blank:?}");
        }
    }
}
