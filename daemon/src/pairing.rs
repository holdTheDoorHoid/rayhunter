//! Which browsers this unit trusts, and how a new one comes to be trusted.
//!
//! A unit fresh from the box knows nobody. On its first start it opens a
//! setup window: for ten minutes it shows a code on its screen, and whoever
//! scans it, chooses the owner passphrase, and gets a device cookie, is the
//! owner. After that the code never appears again. More browsers are added
//! with the passphrase, or later with a short code minted from a browser
//! that is already trusted. Losing every trusted browser is recovered with
//! the passphrase; losing that too is recovered over USB, which is physical
//! possession and already the reset path for everything else.
//!
//! What is stored, in `auth.toml` under the auth directory: the passphrase
//! hash in the same PBKDF2 format as web accounts, a `setup_complete` flag,
//! and one record per trusted device holding the SHA-256 of its cookie
//! token. The tokens themselves are held only by the browsers. A copy of
//! the file, or of a support bundle, therefore lets nobody in.
//!
//! Everything that acts as a credential comes from OS randomness. The setup
//! token is eight characters from the Crockford alphabet, which has no
//! `0/O` or `1/I/L` confusion for anyone typing it off a screen; that is
//! forty bits, and ten wrong tries close the window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::display::SharedOverride;
use crate::display::qr::{self, ScreenGeometry};
use crate::tls::{random_bytes, write_atomic};
use crate::web_auth;

pub const AUTH_FILE: &str = "auth.toml";

/// How long the setup window stays open, on the monotonic clock. The wall
/// clock on these units cannot be trusted and does not enter into it.
pub const SETUP_WINDOW: Duration = Duration::from_secs(10 * 60);
pub const SETUP_TOKEN_LEN: usize = 8;
/// Wrong tokens that close the window early.
pub const MAX_SETUP_ATTEMPTS: u8 = 10;
/// Anything shorter is refused. There are no other rules: a rule nobody
/// asked for is a passphrase nobody remembers.
pub const MIN_PASSPHRASE_LEN: usize = 8;

pub const COOKIE_NAME: &str = "rh_device";
/// A year. There is no server-side expiry; revocation is by hand.
pub const COOKIE_MAX_AGE_SECS: u64 = 365 * 24 * 3600;
/// `last_seen` is written no more than this often, to spare the flash.
pub const LAST_SEEN_INTERVAL: Duration = Duration::from_secs(3600);

/// Crockford's base32 alphabet: no `I`, `L`, `O` or `U`.
pub const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Longest wait a run of wrong passphrases can impose. PBKDF2 already makes
/// each guess cost real time on this hardware; this is the wall behind it.
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// A quiet stretch this long forgets earlier failures.
const BACKOFF_MEMORY: Duration = Duration::from_secs(10 * 60);

/// One browser the unit trusts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustedDevice {
    /// First eight bytes of the token's hash, hex. What the record is looked
    /// up by, so a wrong token costs one hash and one comparison.
    pub id: String,
    pub name: String,
    /// SHA-256 of the cookie token, hex. The token itself is never stored.
    pub token_sha256: String,
    /// RFC 3339, by the unit's wall clock, which may be wrong. Shown, never
    /// relied on.
    pub created: String,
    pub last_seen: String,
    pub user_agent: String,
}

/// Everything persisted. Missing fields read as their defaults, so a file
/// from an older build still loads.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct AuthState {
    pub setup_complete: bool,
    /// `pbkdf2-sha256$…`, the format `web_auth` uses.
    pub owner_passphrase_hash: Option<String>,
    #[serde(rename = "device")]
    pub devices: Vec<TrustedDevice>,
}

/// A trusted device as the interface sees it: no hash.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub created: String,
    pub last_seen: String,
    pub user_agent: String,
    /// Whether this is the browser asking.
    pub current: bool,
}

/// A cookie freshly minted for a browser that has just been trusted.
#[derive(Debug, Clone, PartialEq)]
pub struct IssuedDevice {
    pub id: String,
    pub name: String,
    /// The token as it goes into the cookie. Held by the browser only.
    pub cookie_value: String,
}

impl IssuedDevice {
    /// The `Set-Cookie` header value.
    ///
    /// `Secure`, so the browser only ever sends it over TLS; `HttpOnly`, so
    /// a script cannot read it; `SameSite=Strict`, so another site cannot
    /// make the browser send it. A year, with no server-side expiry.
    pub fn set_cookie_header(&self) -> String {
        format!(
            "{COOKIE_NAME}={}; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}",
            self.cookie_value
        )
    }
}

/// The `Set-Cookie` value that removes the device cookie.
pub fn clear_cookie_header() -> String {
    format!("{COOKIE_NAME}=; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PairError {
    #[error("this unit is already set up")]
    AlreadyComplete,
    #[error("the setup window is closed; press the button on the unit to open it again")]
    WindowClosed,
    #[error("wrong token; {attempts_left} attempts left")]
    WrongToken { attempts_left: u8 },
    #[error("the passphrase must be at least {MIN_PASSPHRASE_LEN} characters")]
    PassphraseTooShort,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("too many wrong attempts; wait {0} seconds")]
    Backoff(u64),
    #[error("no passphrase has been set on this unit")]
    NoPassphrase,
    #[error("no such device")]
    NoSuchDevice,
    #[error("could not save: {0}")]
    Storage(String),
}

/// The setup window while it is open.
#[derive(Debug)]
struct SetupWindow {
    token: String,
    opened: Instant,
    wrong: u8,
}

impl SetupWindow {
    fn expired(&self) -> bool {
        self.opened.elapsed() >= SETUP_WINDOW
    }
}

/// Slows a run of wrong guesses down, unit-wide.
///
/// Unit-wide rather than per address because on a hotspot the guesser
/// chooses their address. Each failure doubles the wait, up to a minute; a
/// success, or ten quiet minutes, forgets the run.
#[derive(Debug, Default)]
pub struct Backoff {
    failures: u32,
    last_failure: Option<Instant>,
}

impl Backoff {
    /// How much longer the next attempt has to wait, if at all.
    pub fn wait_remaining(&self, now: Instant) -> Option<Duration> {
        let last = self.last_failure?;
        if now.duration_since(last) >= BACKOFF_MEMORY {
            return None;
        }
        let delay = Duration::from_secs(1u64 << self.failures.min(6)).min(MAX_BACKOFF);
        (last + delay).checked_duration_since(now)
    }

    pub fn failed(&mut self, now: Instant) {
        if let Some(last) = self.last_failure
            && now.duration_since(last) >= BACKOFF_MEMORY
        {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        self.last_failure = Some(now);
    }

    pub fn succeeded(&mut self) {
        *self = Self::default();
    }
}

/// How the setup code reaches the person: the unit's own screen.
#[derive(Clone)]
pub struct SetupDisplay {
    pub override_: SharedOverride,
    pub screen: ScreenGeometry,
    /// `host:port` the link points at, the hotspot address and TLS port.
    pub host: String,
}

pub struct Pairing {
    /// `None` keeps everything in memory: tests, and a unit whose auth
    /// directory cannot be written, which then pairs only until restart.
    path: Option<PathBuf>,
    state: RwLock<AuthState>,
    setup: Mutex<Option<SetupWindow>>,
    backoff: Mutex<Backoff>,
    last_seen_written: Mutex<HashMap<String, Instant>>,
    display: Option<SetupDisplay>,
}

/// Constant-time equality, so a comparison cannot leak how much of a guess
/// was right.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

/// A fresh setup token.
pub fn new_setup_token() -> Result<String, PairError> {
    let mut raw = [0u8; SETUP_TOKEN_LEN];
    random_bytes(&mut raw).map_err(|e| PairError::Storage(e.to_string()))?;
    Ok(raw
        .iter()
        .map(|b| CROCKFORD[(b & 31) as usize] as char)
        .collect())
}

/// What a person typed, as the token it was meant to be.
///
/// Crockford's rules: case does not matter, `O` is `0`, `I` and `L` are
/// `1`, and separators are ignored. Applied to input only; tokens are
/// always shown in their canonical form.
pub fn normalize_token(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

/// The token in two groups, as it is shown under the code on the screen.
pub fn display_token(token: &str) -> String {
    if token.len() == SETUP_TOKEN_LEN {
        format!("{} {}", &token[..4], &token[4..])
    } else {
        token.to_string()
    }
}

/// A device cookie token and what is stored for it.
fn issue_token() -> Result<(String, String, String), PairError> {
    let mut raw = [0u8; 32];
    random_bytes(&mut raw).map_err(|e| PairError::Storage(e.to_string()))?;
    let value = URL_SAFE_NO_PAD.encode(raw);
    let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    Ok((value, hex(&digest), hex(&digest[..8])))
}

/// A name for a browser nobody has named yet, from its user agent.
pub fn default_device_name(user_agent: &str) -> String {
    let ua = user_agent;
    let platform = if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Mac OS") || ua.contains("Macintosh") {
        "Mac"
    } else if ua.contains("CrOS") {
        "Chromebook"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        "Browser"
    };
    let browser = if ua.contains("Firefox/") {
        Some("Firefox")
    } else if ua.contains("Edg/") {
        Some("Edge")
    } else if ua.contains("OPR/") {
        Some("Opera")
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") {
        Some("Chrome")
    } else if ua.contains("Safari/") {
        Some("Safari")
    } else {
        None
    };
    match browser {
        Some(b) => format!("{platform} ({b})"),
        None => platform.to_string(),
    }
}

/// The value of one cookie out of a `Cookie` header.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k.trim() == name).then_some(v.trim())
    })
}

impl Pairing {
    /// Load the store from `dir`, or start empty if there is none.
    ///
    /// A file that will not parse is a real error: it is not overwritten,
    /// because that would silently throw away the owner's passphrase and
    /// every paired device.
    pub async fn load(dir: &Path, display: Option<SetupDisplay>) -> Result<Self, PairError> {
        let path = dir.join(AUTH_FILE);
        let state = match tokio::fs::read_to_string(&path).await {
            Ok(text) => toml::from_str::<AuthState>(&text)
                .map_err(|e| PairError::Storage(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => AuthState::default(),
            Err(e) => return Err(PairError::Storage(format!("{}: {e}", path.display()))),
        };
        info!(
            "pairing: setup {}, {} trusted device(s)",
            if state.setup_complete {
                "complete"
            } else {
                "not done"
            },
            state.devices.len()
        );
        Ok(Self {
            path: Some(path),
            state: RwLock::new(state),
            setup: Mutex::new(None),
            backoff: Mutex::new(Backoff::default()),
            last_seen_written: Mutex::new(HashMap::new()),
            display,
        })
    }

    /// A store that is never written. For tests, and for a unit that cannot
    /// write its auth directory.
    pub fn ephemeral(state: AuthState, display: Option<SetupDisplay>) -> Self {
        Self {
            path: None,
            state: RwLock::new(state),
            setup: Mutex::new(None),
            backoff: Mutex::new(Backoff::default()),
            last_seen_written: Mutex::new(HashMap::new()),
            display,
        }
    }

    async fn save(&self, state: &AuthState) -> Result<(), PairError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = toml::to_string(state).map_err(|e| PairError::Storage(e.to_string()))?;
        write_atomic(path, text.as_bytes(), 0o600)
            .await
            .map_err(|e| PairError::Storage(e.to_string()))
    }

    pub async fn setup_complete(&self) -> bool {
        self.state.read().await.setup_complete
    }

    pub async fn has_passphrase(&self) -> bool {
        self.state.read().await.owner_passphrase_hash.is_some()
    }

    pub async fn device_count(&self) -> usize {
        self.state.read().await.devices.len()
    }

    /// The open window's token and how long it has left, if one is open.
    pub fn setup_window(&self) -> Option<(String, Duration)> {
        let mut guard = self.setup.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(w) if !w.expired() => Some((
                w.token.clone(),
                SETUP_WINDOW.saturating_sub(w.opened.elapsed()),
            )),
            Some(_) => {
                *guard = None;
                None
            }
            None => None,
        }
    }

    /// Open the setup window, or give an open one its full time again.
    ///
    /// Refused once any device is paired: from then on the code never
    /// appears, and adding devices goes through a trusted browser. A window
    /// that is still open keeps its token, so the code already on the
    /// screen stays right; only the clock restarts.
    pub async fn open_setup_window(&self) -> Result<String, PairError> {
        if self.state.read().await.setup_complete || self.device_count().await > 0 {
            return Err(PairError::AlreadyComplete);
        }
        let token = {
            let mut guard = self.setup.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_mut() {
                Some(w) if !w.expired() => {
                    w.opened = Instant::now();
                    w.token.clone()
                }
                _ => {
                    let token = new_setup_token()?;
                    *guard = Some(SetupWindow {
                        token: token.clone(),
                        opened: Instant::now(),
                        wrong: 0,
                    });
                    token
                }
            }
        };
        self.show_code(&token);
        info!(
            "setup window open for {} minutes",
            SETUP_WINDOW.as_secs() / 60
        );
        Ok(token)
    }

    /// Close the window and take the code off the screen.
    pub fn close_setup_window(&self) {
        let mut guard = self.setup.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
        drop(guard);
        if let Some(display) = &self.display {
            display.override_.clear();
        }
    }

    /// The link the code holds. Uppercase throughout so the QR encoder can
    /// use its compact alphanumeric mode.
    pub fn setup_url(&self, token: &str) -> Option<String> {
        self.display
            .as_ref()
            .map(|d| format!("HTTPS://{}/S/{token}", d.host.to_uppercase()))
    }

    fn show_code(&self, token: &str) {
        let Some(display) = &self.display else {
            warn!("setup window is open but this unit has no screen to show the code on");
            return;
        };
        let Some(url) = self.setup_url(token) else {
            return;
        };
        let Some(code) = qr::encode(&url) else {
            warn!("could not encode the setup link as a QR code");
            return;
        };
        let caption = display_token(token);
        let Some(layout) = qr::layout(code.size() as u32, 4, true, 1, display.screen) else {
            warn!("the setup code does not fit this unit's screen");
            return;
        };
        let pixels = qr::render(&code, layout, Some(&caption), display.screen);
        display.override_.show(pixels, SETUP_WINDOW);
    }

    /// Check a setup token against the open window, counting a wrong one.
    fn check_setup_token(&self, token: &str) -> Result<(), PairError> {
        let mut guard = self.setup.lock().unwrap_or_else(|e| e.into_inner());
        let Some(window) = guard.as_mut() else {
            return Err(PairError::WindowClosed);
        };
        if window.expired() {
            *guard = None;
            return Err(PairError::WindowClosed);
        }
        let given = normalize_token(token);
        if ct_eq(given.as_bytes(), window.token.as_bytes()) {
            return Ok(());
        }
        window.wrong += 1;
        if window.wrong >= MAX_SETUP_ATTEMPTS {
            warn!("{MAX_SETUP_ATTEMPTS} wrong setup tokens; closing the window");
            *guard = None;
            drop(guard);
            if let Some(display) = &self.display {
                display.override_.clear();
            }
            return Err(PairError::WindowClosed);
        }
        Err(PairError::WrongToken {
            attempts_left: MAX_SETUP_ATTEMPTS - window.wrong,
        })
    }

    /// First contact: the scanned token, the owner's chosen passphrase, and
    /// this browser becomes the first trusted device.
    pub async fn complete_setup(
        &self,
        token: &str,
        passphrase: &str,
        device_name: Option<&str>,
        user_agent: &str,
    ) -> Result<IssuedDevice, PairError> {
        if self.state.read().await.setup_complete {
            return Err(PairError::AlreadyComplete);
        }
        self.check_setup_token(token)?;
        if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(PairError::PassphraseTooShort);
        }
        if passphrase.len() > web_auth::MAX_PASSWORD_LEN {
            return Err(PairError::PassphraseTooShort);
        }
        let hash = hash_off_thread(passphrase.to_string()).await;

        let mut state = self.state.write().await;
        let issued = add_device(&mut state, device_name, user_agent)?;
        state.owner_passphrase_hash = Some(hash);
        state.setup_complete = true;
        self.save(&state).await?;
        drop(state);

        self.close_setup_window();
        info!("setup complete; first trusted device is {:?}", issued.name);
        Ok(issued)
    }

    /// Trust this browser because it knows the owner's passphrase.
    pub async fn pair_with_passphrase(
        &self,
        passphrase: &str,
        device_name: Option<&str>,
        user_agent: &str,
    ) -> Result<IssuedDevice, PairError> {
        self.check_backoff()?;
        let Some(hash) = self.state.read().await.owner_passphrase_hash.clone() else {
            return Err(PairError::NoPassphrase);
        };
        if passphrase.len() > web_auth::MAX_PASSWORD_LEN {
            self.note_failure();
            return Err(PairError::WrongPassphrase);
        }
        if !verify_off_thread(passphrase.to_string(), hash).await {
            self.note_failure();
            return Err(PairError::WrongPassphrase);
        }
        self.note_success();
        let mut state = self.state.write().await;
        let issued = add_device(&mut state, device_name, user_agent)?;
        // A unit set up over the old accounts path has a passphrase but may
        // never have been through setup. A browser that knows the
        // passphrase is as good as first contact.
        state.setup_complete = true;
        self.save(&state).await?;
        drop(state);
        self.close_setup_window();
        info!("paired {:?} with the passphrase", issued.name);
        Ok(issued)
    }

    /// Trust this browser because it knows a web account from before
    /// pairing existed, and make that account's password the owner
    /// passphrase if there is none yet. The migration path for units
    /// already in the field.
    pub async fn pair_with_account(
        &self,
        users: &[web_auth::WebUser],
        username: &str,
        password: &str,
        device_name: Option<&str>,
        user_agent: &str,
    ) -> Result<IssuedDevice, PairError> {
        self.check_backoff()?;
        let users = users.to_vec();
        let users_for_check = users.clone();
        let (username_owned, password_owned) = (username.to_string(), password.to_string());
        let valid = tokio::task::spawn_blocking(move || {
            web_auth::credentials_are_valid(&users_for_check, &username_owned, &password_owned)
        })
        .await
        .unwrap_or(false);
        if !valid {
            self.note_failure();
            return Err(PairError::WrongPassphrase);
        }
        self.note_success();
        let hash = users
            .iter()
            .find(|u| u.username == username)
            .map(|u| u.password_hash.clone());
        let mut state = self.state.write().await;
        let issued = add_device(&mut state, device_name, user_agent)?;
        if state.owner_passphrase_hash.is_none() {
            state.owner_passphrase_hash = hash;
        }
        state.setup_complete = true;
        self.save(&state).await?;
        drop(state);
        self.close_setup_window();
        info!("paired {:?} with account {username:?}", issued.name);
        Ok(issued)
    }

    fn check_backoff(&self) -> Result<(), PairError> {
        let backoff = self.backoff.lock().unwrap_or_else(|e| e.into_inner());
        match backoff.wait_remaining(Instant::now()) {
            Some(left) => Err(PairError::Backoff(left.as_secs().max(1))),
            None => Ok(()),
        }
    }

    fn note_failure(&self) {
        self.backoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .failed(Instant::now());
    }

    fn note_success(&self) {
        self.backoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .succeeded();
    }

    /// The device a cookie token belongs to, if any.
    ///
    /// One hash, one lookup by prefix, one constant-time comparison. The
    /// record's `last_seen` is refreshed at most hourly, in the background.
    pub async fn authenticate(&self, cookie_token: &str) -> Option<TrustedDevice> {
        if cookie_token.is_empty() || cookie_token.len() > 128 {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(cookie_token.as_bytes()).into();
        let id = hex(&digest[..8]);
        let wanted = hex(&digest);
        let device = {
            let state = self.state.read().await;
            let found = state.devices.iter().find(|d| d.id == id)?;
            if !ct_eq(found.token_sha256.as_bytes(), wanted.as_bytes()) {
                return None;
            }
            found.clone()
        };
        self.touch(&device.id).await;
        Some(device)
    }

    async fn touch(&self, id: &str) {
        let due = {
            let mut seen = self
                .last_seen_written
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match seen.get(id) {
                Some(at) if at.elapsed() < LAST_SEEN_INTERVAL => false,
                _ => {
                    seen.insert(id.to_string(), Instant::now());
                    true
                }
            }
        };
        if !due {
            return;
        }
        let mut state = self.state.write().await;
        if let Some(d) = state.devices.iter_mut().find(|d| d.id == id) {
            d.last_seen = now_rfc3339();
        }
        if let Err(e) = self.save(&state).await {
            warn!("could not record last_seen: {e}");
        }
    }

    pub async fn devices(&self, current_id: Option<&str>) -> Vec<DeviceInfo> {
        self.state
            .read()
            .await
            .devices
            .iter()
            .map(|d| DeviceInfo {
                id: d.id.clone(),
                name: d.name.clone(),
                created: d.created.clone(),
                last_seen: d.last_seen.clone(),
                user_agent: d.user_agent.clone(),
                current: current_id == Some(d.id.as_str()),
            })
            .collect()
    }

    pub async fn rename_device(&self, id: &str, name: &str) -> Result<(), PairError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(PairError::NoSuchDevice);
        }
        let mut state = self.state.write().await;
        let Some(d) = state.devices.iter_mut().find(|d| d.id == id) else {
            return Err(PairError::NoSuchDevice);
        };
        d.name = name.to_string();
        self.save(&state).await
    }

    /// Forget a device. Its cookie stops working at once.
    pub async fn revoke_device(&self, id: &str) -> Result<(), PairError> {
        let mut state = self.state.write().await;
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before {
            return Err(PairError::NoSuchDevice);
        }
        self.save(&state).await?;
        info!("revoked device {id}");
        Ok(())
    }
}

fn add_device(
    state: &mut AuthState,
    device_name: Option<&str>,
    user_agent: &str,
) -> Result<IssuedDevice, PairError> {
    let (cookie_value, token_sha256, id) = issue_token()?;
    let name = device_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(64).collect::<String>())
        .unwrap_or_else(|| default_device_name(user_agent));
    let now = now_rfc3339();
    state.devices.push(TrustedDevice {
        id: id.clone(),
        name: name.clone(),
        token_sha256,
        created: now.clone(),
        last_seen: now,
        user_agent: user_agent.chars().take(256).collect(),
    });
    Ok(IssuedDevice {
        id,
        name,
        cookie_value,
    })
}

/// PBKDF2 is slow by design and this runtime has one thread; hashing inline
/// would stall every other request for the duration.
async fn hash_off_thread(passphrase: String) -> String {
    tokio::task::spawn_blocking(move || web_auth::hash_password(&passphrase))
        .await
        .expect("hashing does not panic")
}

async fn verify_off_thread(passphrase: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || web_auth::verify_password(&passphrase, &hash))
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";

    fn fresh() -> Pairing {
        Pairing::ephemeral(AuthState::default(), None)
    }

    #[test]
    fn setup_tokens_use_the_confusion_free_alphabet() {
        for _ in 0..50 {
            let t = new_setup_token().unwrap();
            assert_eq!(t.len(), SETUP_TOKEN_LEN);
            assert!(t.bytes().all(|b| CROCKFORD.contains(&b)), "{t}");
            assert!(!t.contains(['I', 'L', 'O', 'U']), "{t}");
        }
        assert_ne!(new_setup_token().unwrap(), new_setup_token().unwrap());
    }

    /// Whatever a person types off the screen, within reason, is the token.
    #[test]
    fn typed_tokens_are_forgiven_their_usual_mistakes() {
        assert_eq!(normalize_token("7k3m 9xwq"), "7K3M9XWQ");
        assert_eq!(normalize_token("7K3M-9XWQ"), "7K3M9XWQ");
        assert_eq!(normalize_token("O1IL"), "0111");
        assert_eq!(display_token("7K3M9XWQ"), "7K3M 9XWQ");
    }

    #[tokio::test]
    async fn first_contact_makes_the_owner_and_the_first_device() {
        let p = fresh();
        assert!(!p.setup_complete().await);
        let token = p.open_setup_window().await.unwrap();
        assert!(p.setup_window().is_some());

        let issued = p
            .complete_setup(&token.to_lowercase(), "correct horse", None, UA)
            .await
            .unwrap();
        assert_eq!(issued.name, "iPhone (Safari)");
        assert!(p.setup_complete().await);
        assert!(p.has_passphrase().await);
        assert_eq!(p.device_count().await, 1);
        assert!(p.setup_window().is_none(), "the window closes on success");

        // The cookie the browser got is the credential.
        let dev = p.authenticate(&issued.cookie_value).await.unwrap();
        assert_eq!(dev.id, issued.id);
        assert!(p.authenticate("not-a-token").await.is_none());
        assert!(p.authenticate("").await.is_none());

        // And it can never be done twice.
        assert_eq!(
            p.complete_setup(&token, "correct horse", None, UA).await,
            Err(PairError::AlreadyComplete)
        );
        assert_eq!(p.open_setup_window().await, Err(PairError::AlreadyComplete));
    }

    #[tokio::test]
    async fn the_cookie_is_locked_down() {
        let p = fresh();
        let token = p.open_setup_window().await.unwrap();
        let issued = p
            .complete_setup(&token, "correct horse", Some("My phone"), UA)
            .await
            .unwrap();
        let header = issued.set_cookie_header();
        for attr in [
            "Secure",
            "HttpOnly",
            "SameSite=Strict",
            "Path=/",
            "Max-Age=31536000",
        ] {
            assert!(header.contains(attr), "{header}");
        }
        assert!(header.starts_with("rh_device="));
        assert_eq!(issued.name, "My phone");
        assert_eq!(
            cookie_value("a=1; rh_device=abc; b=2", COOKIE_NAME),
            Some("abc")
        );
        assert_eq!(cookie_value("a=1", COOKIE_NAME), None);
    }

    #[tokio::test]
    async fn wrong_tokens_are_counted_and_close_the_window() {
        let p = fresh();
        let token = p.open_setup_window().await.unwrap();
        for i in 1..MAX_SETUP_ATTEMPTS {
            assert_eq!(
                p.complete_setup("WRONGONE", "correct horse", None, UA)
                    .await,
                Err(PairError::WrongToken {
                    attempts_left: MAX_SETUP_ATTEMPTS - i
                })
            );
        }
        assert_eq!(
            p.complete_setup("WRONGONE", "correct horse", None, UA)
                .await,
            Err(PairError::WindowClosed)
        );
        // Even the right token is no good now.
        assert_eq!(
            p.complete_setup(&token, "correct horse", None, UA).await,
            Err(PairError::WindowClosed)
        );
        assert!(p.setup_window().is_none());
        // The button opens it again with a new token.
        let again = p.open_setup_window().await.unwrap();
        assert_ne!(again, token);
    }

    #[tokio::test]
    async fn a_short_passphrase_is_refused_without_consuming_the_window() {
        let p = fresh();
        let token = p.open_setup_window().await.unwrap();
        assert_eq!(
            p.complete_setup(&token, "short", None, UA).await,
            Err(PairError::PassphraseTooShort)
        );
        assert!(p.setup_window().is_some());
        assert!(
            p.complete_setup(&token, "long enough", None, UA)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn rearming_keeps_the_token_on_the_screen() {
        let p = fresh();
        let first = p.open_setup_window().await.unwrap();
        let second = p.open_setup_window().await.unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn the_passphrase_adds_devices_and_wrong_ones_slow_down() {
        let p = fresh();
        assert_eq!(
            p.pair_with_passphrase("anything", None, UA).await,
            Err(PairError::NoPassphrase)
        );
        let token = p.open_setup_window().await.unwrap();
        p.complete_setup(&token, "correct horse", None, UA)
            .await
            .unwrap();

        let laptop = p
            .pair_with_passphrase(
                "correct horse",
                Some("Laptop"),
                "Mozilla/5.0 (X11; Linux x86_64) Firefox/130.0",
            )
            .await
            .unwrap();
        assert_eq!(p.device_count().await, 2);
        assert!(p.authenticate(&laptop.cookie_value).await.is_some());

        assert_eq!(
            p.pair_with_passphrase("wrong horse", None, UA).await,
            Err(PairError::WrongPassphrase)
        );
        // Straight after a failure the next try has to wait.
        assert!(matches!(
            p.pair_with_passphrase("correct horse", None, UA).await,
            Err(PairError::Backoff(_))
        ));
    }

    #[test]
    fn backoff_doubles_caps_and_forgets() {
        let mut b = Backoff::default();
        let t0 = Instant::now();
        assert_eq!(b.wait_remaining(t0), None);
        b.failed(t0);
        let w1 = b.wait_remaining(t0).unwrap();
        assert!(w1 <= Duration::from_secs(2) && w1 > Duration::from_secs(1));
        b.failed(t0);
        let w2 = b.wait_remaining(t0).unwrap();
        assert!(w2 > w1);
        for _ in 0..20 {
            b.failed(t0);
        }
        assert!(b.wait_remaining(t0).unwrap() <= MAX_BACKOFF);
        // A long quiet spell forgets it all.
        assert_eq!(b.wait_remaining(t0 + BACKOFF_MEMORY), None);
        b.succeeded();
        assert_eq!(b.wait_remaining(t0), None);
    }

    #[tokio::test]
    async fn an_old_account_pairs_and_becomes_the_passphrase() {
        let p = fresh();
        let users = vec![web_auth::WebUser {
            username: "alice".into(),
            password_hash: web_auth::hash_password("alpha-bravo"),
        }];
        assert_eq!(
            p.pair_with_account(&users, "alice", "wrong", None, UA)
                .await,
            Err(PairError::WrongPassphrase)
        );
        p.note_success();
        let issued = p
            .pair_with_account(&users, "alice", "alpha-bravo", None, UA)
            .await
            .unwrap();
        assert!(p.setup_complete().await);
        assert!(p.authenticate(&issued.cookie_value).await.is_some());
        // The account's password now works as the owner passphrase.
        assert!(
            p.pair_with_passphrase("alpha-bravo", None, UA)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn devices_are_listed_renamed_and_revoked() {
        let p = fresh();
        let token = p.open_setup_window().await.unwrap();
        let a = p
            .complete_setup(&token, "correct horse", None, UA)
            .await
            .unwrap();
        let b = p
            .pair_with_passphrase("correct horse", Some("Second"), UA)
            .await
            .unwrap();

        let list = p.devices(Some(&a.id)).await;
        assert_eq!(list.len(), 2);
        assert!(list.iter().find(|d| d.id == a.id).unwrap().current);
        assert!(!list.iter().find(|d| d.id == b.id).unwrap().current);

        p.rename_device(&b.id, "  Kitchen tablet ").await.unwrap();
        assert_eq!(
            p.devices(None)
                .await
                .iter()
                .find(|d| d.id == b.id)
                .unwrap()
                .name,
            "Kitchen tablet"
        );
        assert_eq!(
            p.rename_device(&b.id, "   ").await,
            Err(PairError::NoSuchDevice)
        );

        p.revoke_device(&b.id).await.unwrap();
        assert!(p.authenticate(&b.cookie_value).await.is_none());
        assert!(p.authenticate(&a.cookie_value).await.is_some());
        assert_eq!(p.revoke_device(&b.id).await, Err(PairError::NoSuchDevice));
    }

    /// The store round-trips through the file and nothing secret is in it.
    #[tokio::test]
    async fn the_store_survives_a_restart_and_holds_no_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let p = Pairing::load(dir.path(), None).await.unwrap();
        let token = p.open_setup_window().await.unwrap();
        let issued = p
            .complete_setup(&token, "correct horse", None, UA)
            .await
            .unwrap();

        let text = std::fs::read_to_string(dir.path().join(AUTH_FILE)).unwrap();
        assert!(text.contains("setup_complete = true"));
        assert!(text.contains("pbkdf2-sha256$"));
        assert!(
            !text.contains(&issued.cookie_value),
            "the token must not be stored"
        );
        assert!(!text.contains("correct horse"));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(AUTH_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let again = Pairing::load(dir.path(), None).await.unwrap();
        assert!(again.setup_complete().await);
        assert!(again.authenticate(&issued.cookie_value).await.is_some());
        assert_eq!(
            again.open_setup_window().await,
            Err(PairError::AlreadyComplete)
        );
    }

    /// A file that does not parse must not be quietly replaced: that would
    /// throw away the owner's passphrase and every paired device.
    #[tokio::test]
    async fn a_corrupt_store_is_an_error_not_a_reset() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(AUTH_FILE), "this is not = [toml").unwrap();
        assert!(matches!(
            Pairing::load(dir.path(), None).await,
            Err(PairError::Storage(_))
        ));
    }

    #[test]
    fn device_names_come_from_the_user_agent() {
        assert_eq!(default_device_name(UA), "iPhone (Safari)");
        assert_eq!(
            default_device_name(
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36"
            ),
            "Android (Chrome)"
        );
        assert_eq!(
            default_device_name(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:130.0) Gecko/20100101 Firefox/130.0"
            ),
            "Windows (Firefox)"
        );
        assert_eq!(default_device_name("curl/8.5.0"), "Browser");
    }
}
