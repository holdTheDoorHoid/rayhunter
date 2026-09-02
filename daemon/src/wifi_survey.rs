//! A survey of the access points around the device, with configurable alerts.
//!
//! This is the honest half of the wireless-surveillance work. Monitor mode is
//! refused by the firmware on every Orbic variant tested, so the device cannot
//! see probe requests or any device that is only listening. What it *can* do is
//! a full BSS scan, which returns every access point transmitting nearby, with
//! its BSSID, channel, signal and security. That is what this serves.
//!
//! Stating the limit matters more than the feature: a device that is not
//! beaconing is invisible here, so an empty result means "nothing is
//! broadcasting", never "nothing is watching".
//!
//! Scans run on whichever wireless interface exists. On the Unisoc variant the
//! chip permits only one managed-or-AP interface, so the scan runs on the live
//! hotspot interface; that is safe — hostapd keeps running through it — but it
//! is why this does not create its own interface the way a monitor-mode
//! implementation would.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use rayhunter_radio::observation::{ObservationPayload, WifiObservation};
use rayhunter_radio::scan::parse_iw_scan;
use rayhunter_radio::signature::{Confidence, Detection, Severity, SignatureDb};
use rayhunter_radio::userrules::UserRuleSet;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::server::ServerState;

/// The curated pack, compiled in. User rules live in a separate file so that
/// editing one can never corrupt the other.
const BUILTIN_PACK: &str =
    include_str!("../../radio/signatures/builtin-surveillance-signatures.json");

/// A scan takes a few seconds and briefly occupies the radio. Refuse to start
/// another within this window rather than queue them up on a device with one
/// slow core.
const MIN_SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// Scanning is quick, but a wedged `iw` should not hold the slot for ever.
const SCAN_TIMEOUT: Duration = Duration::from_secs(30);

/// Ceiling on the rules file, which arrives from a browser.
const MAX_RULES_BYTES: usize = 256 * 1024;

#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
/// One access point as the UI shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyEntry {
    pub bssid: Option<String>,
    pub ssid: Option<String>,
    /// True when the SSID element was present but empty: a hidden network.
    pub hidden: bool,
    pub frequency_mhz: Option<u32>,
    pub channel: Option<u16>,
    pub band: Option<String>,
    pub signal_dbm: Option<i16>,
    /// Which information elements were present, by name where known.
    pub elements: Vec<u8>,
    /// "Open", "WPA2", "WPA2/WPA3 Enterprise" and so on.
    pub security: Option<String>,
    /// True for open or WEP: anyone can read this network's traffic.
    pub unprotected: bool,
    /// WPS advertised, a known weak point.
    pub wps: bool,
    /// Milliseconds since the network was last heard.
    pub last_seen_ms: Option<u32>,
    /// Alerts this access point matched, strongest first.
    pub alerts: Vec<Detection>,
    /// True when the address is locally administered, i.e. randomised or
    /// otherwise not a manufacturer's. Vendor lookups mean nothing on these.
    pub randomised_address: bool,
}

#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyResponse {
    pub scanned_at: String,
    pub interface: String,
    pub networks: Vec<SurveyEntry>,
    /// How many of `networks` matched at least one alert.
    pub alerting: usize,
    /// Rules in force, so the panel can say what it is matching against.
    pub builtin_rules_enabled: usize,
    pub user_rules_enabled: usize,
    /// Said plainly in the UI rather than left for the user to infer.
    pub limitations: Vec<String>,
}

#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesResponse {
    pub rules: UserRuleSet,
    /// Names of the builtin signatures, for display. Not editable here.
    pub builtin: Vec<BuiltinSummary>,
}

#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinSummary {
    pub id: String,
    pub vendor: String,
    pub description: String,
    /// Effective state: the pack's own flag, or the user's override of it.
    pub enabled: bool,
    /// True when the user has overridden the shipped default.
    pub overridden: bool,
    pub confidence: Confidence,
    pub severity: Severity,
    /// False when the rule cannot fire on the only capture this device has.
    /// Shown so a rule that is on but structurally unable to match is not
    /// mistaken for coverage.
    pub reachable: bool,
    /// Provenance has not been independently checked.
    pub unverified: bool,
    pub notes: Option<String>,
}

/// Only one scan at a time, and not more often than `MIN_SCAN_INTERVAL`.
static SCAN_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
static LAST_SCAN: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

fn rules_path(config_path: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(config_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/data/rayhunter"));
    dir.join("wifi-alert-rules.json")
}

/// The first interface with a wireless PHY.
///
/// Deliberately not hard-coded: the RC400L ships with at least three different
/// Wi-Fi chips across production runs, and the interface names differ with
/// them.
async fn find_wireless_interface() -> Option<String> {
    let mut dir = tokio::fs::read_dir("/sys/class/net").await.ok()?;
    let mut found: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if tokio::fs::metadata(format!("/sys/class/net/{name}/phy80211"))
            .await
            .is_ok()
        {
            found.push(name);
        }
    }
    found.sort();
    found.into_iter().next()
}

async fn load_user_rules(config_path: &str) -> UserRuleSet {
    let path = rules_path(config_path);
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => match UserRuleSet::from_json(&text) {
            Ok(set) => set,
            Err(e) => {
                log::warn!("ignoring unusable {}: {e}", path.display());
                UserRuleSet::default()
            }
        },
        // Absent is the normal state before anyone adds a rule.
        Err(_) => UserRuleSet::default(),
    }
}

fn builtin_db() -> SignatureDb {
    SignatureDb::from_json(BUILTIN_PACK).unwrap_or_else(|e| {
        // The pack is compiled in and covered by tests, so this cannot happen
        // in a build that passed CI. Degrade to user rules rather than refuse
        // to serve the panel at all.
        log::error!("builtin signature pack failed to parse: {e}");
        SignatureDb::empty()
    })
}

fn band_of(freq: u32) -> Option<String> {
    match freq {
        2400..=2500 => Some("2.4 GHz".to_string()),
        4900..=5900 => Some("5 GHz".to_string()),
        5925..=7125 => Some("6 GHz".to_string()),
        _ => None,
    }
}

fn to_entry(obs: &WifiObservation, db: &SignatureDb) -> SurveyEntry {
    let payload = ObservationPayload::Wifi(obs.clone());
    let mut alerts = db.match_observation(&payload);
    // Strongest first, so the UI can show the worst without sorting again.
    alerts.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then(b.severity.cmp(&a.severity))
    });

    let ssid_display = obs.ssid.as_ref().map(|s| s.display());
    let hidden = obs.ssid.as_ref().map(|s| s.is_wildcard()).unwrap_or(false);

    SurveyEntry {
        bssid: obs.bssid.map(|m| m.to_string()),
        // An empty string here means hidden, which `hidden` already says; keep
        // the field null so the UI does not print an empty name.
        ssid: ssid_display.filter(|s| !s.is_empty()),
        hidden,
        frequency_mhz: obs.frequency_mhz,
        channel: obs.channel(),
        band: obs.frequency_mhz.and_then(band_of),
        signal_dbm: obs.rssi_dbm,
        elements: obs.information_elements.iter().map(|ie| ie.id).collect(),
        security: obs.security.map(|s| s.label()),
        unprotected: obs.security.map(|s| s.is_unprotected()).unwrap_or(false),
        wps: obs.wps,
        last_seen_ms: obs.last_seen_ms,
        randomised_address: obs
            .bssid
            .map(|m| m.is_locally_administered())
            .unwrap_or(false),
        alerts,
    }
}

/// Apply the user's per-signature overrides to the builtin pack.
///
/// The pack is compiled in and replaced wholesale by an update, so an override
/// is stored separately and reapplied rather than the pack being edited.
fn apply_overrides(db: &mut SignatureDb, user: &UserRuleSet) {
    for sig in db.signatures.iter_mut() {
        if let Some(&want) = user.builtin_overrides.get(&sig.id) {
            sig.enabled = want;
        }
    }
}

fn summarise_builtin(user: &UserRuleSet) -> Vec<BuiltinSummary> {
    let mut db = builtin_db();
    let shipped: Vec<bool> = db.signatures.iter().map(|s| s.enabled).collect();
    apply_overrides(&mut db, user);
    db.signatures
        .into_iter()
        .zip(shipped)
        .map(|(s, was)| BuiltinSummary {
            reachable: s.reachable_via_bss_scan(),
            unverified: s.last_verified.is_none(),
            overridden: s.enabled != was,
            id: s.id,
            vendor: s.vendor,
            description: s.description,
            enabled: s.enabled,
            confidence: s.confidence,
            severity: s.severity,
            notes: s.notes,
        })
        .collect()
}

/// Run a scan and return what is on the air, with any alerts matched.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/wifi/survey",
    tag = "Wireless",
    responses(
        (status = StatusCode::OK, description = "Scan completed", body = SurveyResponse),
        (status = StatusCode::TOO_MANY_REQUESTS, description = "A scan is already running, or one finished moments ago"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "No wireless interface on this device"),
    ),
    summary = "Survey nearby access points",
))]
pub async fn wifi_survey(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<SurveyResponse>, (StatusCode, String)> {
    let Ok(_slot) = SCAN_SLOT.try_acquire() else {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "a scan is already running".to_string(),
        ));
    };
    {
        let last = LAST_SCAN.lock().unwrap();
        if let Some(when) = *last {
            let since = when.elapsed();
            if since < MIN_SCAN_INTERVAL {
                let wait = (MIN_SCAN_INTERVAL - since).as_secs() + 1;
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("scanned moments ago; try again in {wait}s"),
                ));
            }
        }
    }

    let Some(iface) = find_wireless_interface().await else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "no wireless interface on this device".to_string(),
        ));
    };

    let output = tokio::time::timeout(
        SCAN_TIMEOUT,
        tokio::process::Command::new("iw")
            .args(["dev", &iface, "scan"])
            .kill_on_drop(true)
            .output(),
    )
    .await;

    *LAST_SCAN.lock().unwrap() = Some(Instant::now());

    let output = match output {
        Err(_) => {
            return Err((
                StatusCode::GATEWAY_TIMEOUT,
                "the scan did not finish in time".to_string(),
            ));
        }
        Ok(Err(e)) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not run iw: {e}"),
            ));
        }
        Ok(Ok(o)) => o,
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("scan failed on {iface}: {}", err.trim()),
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let observations = parse_iw_scan(&text);

    let user = load_user_rules(&state.config_path).await;
    let mut db = builtin_db();
    apply_overrides(&mut db, &user);
    let builtin_enabled = db.signatures.iter().filter(|s| s.enabled).count();
    let user_enabled = match user.to_signatures() {
        Ok(sigs) => {
            let n = sigs.len();
            db.signatures.extend(sigs);
            n
        }
        Err(e) => {
            log::warn!("user alert rules unusable, ignoring them: {e}");
            0
        }
    };

    let networks: Vec<SurveyEntry> = observations
        .iter()
        .filter(|o| {
            // Anything the user has explicitly silenced never reaches the UI.
            o.bssid.map(|b| !user.is_allowlisted(&b)).unwrap_or(true)
        })
        .map(|o| to_entry(o, &db))
        .collect();
    let alerting = networks.iter().filter(|n| !n.alerts.is_empty()).count();

    Ok(Json(SurveyResponse {
        scanned_at: chrono::Local::now().to_rfc3339(),
        interface: iface,
        alerting,
        networks,
        builtin_rules_enabled: builtin_enabled,
        user_rules_enabled: user_enabled,
        limitations: vec![
            "Only access points that are transmitting appear here. A device that is \
             merely listening, or that only sends probe requests, cannot be seen."
                .to_string(),
            "Monitor mode is refused by this device's Wi-Fi firmware, so probe requests \
             cannot be captured. An empty result means nothing is broadcasting nearby, \
             not that nothing is watching."
                .to_string(),
            "A vendor match is weak evidence on its own. Addresses can be set to \
             anything, and randomised ones are marked as such."
                .to_string(),
        ],
    }))
}

/// The user's alert rules, plus a read-only view of the builtin ones.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/wifi/alert-rules",
    tag = "Wireless",
    responses((status = StatusCode::OK, description = "Current rules", body = RulesResponse)),
    summary = "Read the Wi-Fi alert rules",
))]
pub async fn get_wifi_rules(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<RulesResponse>, (StatusCode, String)> {
    let rules = load_user_rules(&state.config_path).await;
    let builtin = summarise_builtin(&rules);
    Ok(Json(RulesResponse { rules, builtin }))
}

/// Replace the user's alert rules.
///
/// Validated before anything is written: a rejected set leaves the previous one
/// in place, so a bad edit in the browser cannot disarm the alerts.
#[cfg_attr(feature = "apidocs", utoipa::path(
    post,
    path = "/api/wifi/alert-rules",
    tag = "Wireless",
    request_body(content = UserRuleSet, description = "The complete rule set, replacing what is stored"),
    responses(
        (status = StatusCode::OK, description = "Rules stored"),
        (status = StatusCode::BAD_REQUEST, description = "Rules rejected; the previous set is unchanged"),
    ),
    summary = "Replace the Wi-Fi alert rules",
))]
pub async fn set_wifi_rules(
    State(state): State<Arc<ServerState>>,
    Json(rules): Json<UserRuleSet>,
) -> Result<Json<RulesResponse>, (StatusCode, String)> {
    rules
        .validate()
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let text = serde_json::to_string_pretty(&rules)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if text.len() > MAX_RULES_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("rule set is larger than the {MAX_RULES_BYTES} byte limit"),
        ));
    }

    let path = rules_path(&state.config_path);
    // Write beside the target and rename, so an interrupted write cannot leave
    // a half-file that fails to parse on the next boot and silently disarms
    // every user rule.
    let tmp = path.with_extension("json.new");
    tokio::fs::write(&tmp, &text)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    get_wifi_rules(State(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayhunter_radio::MacAddr;

    const REAL_SCAN: &str = r#"BSS 74:90:bc:b7:36:0d(on wlan0)
	freq: 2412
	signal: -73.00 dBm
	SSID: Jan's House
	DS Parameter set: channel 1
	RSN:	 * Version: 1
BSS b4:1e:52:11:22:33(on wlan0)
	freq: 5745
	signal: -60.00 dBm
	SSID: FS-camera
BSS da:a1:19:00:00:01(on wlan0)
	freq: 2437
	signal: -80.00 dBm
	SSID:
"#;

    #[test]
    fn survey_entries_carry_channel_band_and_hidden_state() {
        let db = builtin_db();
        let obs = parse_iw_scan(REAL_SCAN);
        let entries: Vec<SurveyEntry> = obs.iter().map(|o| to_entry(o, &db)).collect();
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].ssid.as_deref(), Some("Jan's House"));
        assert_eq!(entries[0].channel, Some(1));
        assert_eq!(entries[0].band.as_deref(), Some("2.4 GHz"));
        assert_eq!(entries[0].signal_dbm, Some(-73));
        assert!(!entries[0].hidden);

        assert_eq!(entries[1].band.as_deref(), Some("5 GHz"));
        assert_eq!(entries[1].channel, Some(149));

        // Zero-length SSID is a hidden network, and must not show as a blank name.
        assert!(entries[2].hidden);
        assert_eq!(entries[2].ssid, None);
    }

    #[test]
    fn a_known_vendor_prefix_raises_an_alert() {
        let db = builtin_db();
        let obs = parse_iw_scan(REAL_SCAN);
        let entries: Vec<SurveyEntry> = obs.iter().map(|o| to_entry(o, &db)).collect();
        // b4:1e:52 is Flock Safety in the builtin pack.
        let flock = &entries[1];
        assert_eq!(flock.alerts.len(), 1);
        assert_eq!(flock.alerts[0].vendor, "Flock Safety");
        // And it is weak evidence, which the UI must be able to say.
        assert_eq!(flock.alerts[0].confidence, Confidence::Info);
    }

    #[test]
    fn randomised_addresses_are_flagged_and_do_not_match_vendor_rules() {
        let db = builtin_db();
        let obs = parse_iw_scan(REAL_SCAN);
        let entries: Vec<SurveyEntry> = obs.iter().map(|o| to_entry(o, &db)).collect();
        let randomised = &entries[2];
        assert!(randomised.randomised_address);
        assert!(randomised.alerts.is_empty());
    }

    #[test]
    fn ordinary_networks_raise_nothing() {
        let db = builtin_db();
        let obs = parse_iw_scan(REAL_SCAN);
        let entries: Vec<SurveyEntry> = obs.iter().map(|o| to_entry(o, &db)).collect();
        assert!(entries[0].alerts.is_empty());
    }

    #[test]
    fn user_rules_add_alerts_alongside_the_builtin_ones() {
        let json = r#"{
            "rules": [{
                "id": "vanrule", "name": "The white van",
                "technology": "wifi", "severity": "medium",
                "criteria": [{"type": "ssid_contains", "substring": "camera"}]
            }],
            "allowlist": []
        }"#;
        let user = UserRuleSet::from_json(json).unwrap();
        let mut db = builtin_db();
        db.signatures.extend(user.to_signatures().unwrap());

        let obs = parse_iw_scan(REAL_SCAN);
        let entries: Vec<SurveyEntry> = obs.iter().map(|o| to_entry(o, &db)).collect();
        // FS-camera matches both the Flock prefix and the user's SSID rule.
        let ids: Vec<&str> = entries[1]
            .alerts
            .iter()
            .map(|a| a.signature_id.as_str())
            .collect();
        assert!(ids.contains(&"user.vanrule"), "got {ids:?}");
        assert!(
            ids.iter().any(|i| i.starts_with("camera.flock")),
            "got {ids:?}"
        );
    }

    #[test]
    fn allowlisted_devices_can_be_silenced() {
        let json = r#"{
            "rules": [],
            "allowlist": [{"prefix": "b4:1e:52", "label": "my own test camera"}]
        }"#;
        let user = UserRuleSet::from_json(json).unwrap();
        assert!(user.is_allowlisted(&MacAddr::parse("b4:1e:52:11:22:33").unwrap()));
        assert!(!user.is_allowlisted(&MacAddr::parse("74:90:bc:b7:36:0d").unwrap()));
    }

    #[test]
    fn overrides_change_which_builtin_rules_fire() {
        // Silence the Flock prefix rule and it should stop matching.
        let json = r#"{
            "rules": [], "allowlist": [],
            "builtin_overrides": {"camera.flock.oui": false}
        }"#;
        let user = UserRuleSet::from_json(json).unwrap();

        let mut db = builtin_db();
        let before = db.match_observation(&ObservationPayload::Wifi({
            let mut o = WifiObservation::empty();
            o.bssid = Some(rayhunter_radio::MacAddr::parse("b4:1e:52:11:22:33").unwrap());
            o
        }));
        assert!(
            !before.is_empty(),
            "the Flock prefix should match by default"
        );

        apply_overrides(&mut db, &user);
        let after = db.match_observation(&ObservationPayload::Wifi({
            let mut o = WifiObservation::empty();
            o.bssid = Some(rayhunter_radio::MacAddr::parse("b4:1e:52:11:22:33").unwrap());
            o
        }));
        assert!(after.is_empty(), "the override should have silenced it");
    }

    #[test]
    fn the_summary_reports_reachability_and_override_state() {
        let json = r#"{
            "rules": [], "allowlist": [],
            "builtin_overrides": {"camera.flock.oui": false}
        }"#;
        let user = UserRuleSet::from_json(json).unwrap();
        let summary = summarise_builtin(&user);

        let flock = summary.iter().find(|b| b.id == "camera.flock.oui").unwrap();
        assert!(!flock.enabled);
        assert!(flock.overridden, "a user-changed rule should say so");
        assert!(flock.reachable);

        // The probe-request rules ship on but cannot fire on this hardware.
        let probe = summary
            .iter()
            .find(|b| b.id == "research.flock.nitekry.wildcard-probe")
            .unwrap();
        assert!(probe.enabled);
        assert!(
            !probe.reachable,
            "needs monitor mode, so it cannot fire here"
        );
        assert!(probe.unverified);
        assert!(!probe.overridden);
    }

    #[test]
    fn security_and_wps_reach_the_survey_entry() {
        let scan = "BSS aa:bb:cc:00:00:01(on wlan0)\n\tcapability: ESS (0x1001)\n\tlast seen: 90 ms ago\n\tSSID: OpenNet\n";
        let db = builtin_db();
        let entries: Vec<SurveyEntry> = parse_iw_scan(scan)
            .iter()
            .map(|o| to_entry(o, &db))
            .collect();
        assert_eq!(entries[0].security.as_deref(), Some("Open"));
        assert!(entries[0].unprotected);
        assert!(!entries[0].wps);
        assert_eq!(entries[0].last_seen_ms, Some(90));
    }

    #[test]
    fn rules_path_sits_beside_the_config() {
        let p = rules_path("/data/rayhunter/config.toml");
        assert_eq!(
            p,
            std::path::Path::new("/data/rayhunter/wifi-alert-rules.json")
        );
    }

    #[test]
    fn bands_are_named_for_display() {
        assert_eq!(band_of(2412).as_deref(), Some("2.4 GHz"));
        assert_eq!(band_of(5745).as_deref(), Some("5 GHz"));
        assert_eq!(band_of(60_000), None);
    }
}
