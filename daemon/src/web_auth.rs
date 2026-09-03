//! Passwords for the web interface.
//!
//! Rayhunter's web interface has historically had no authentication at all.
//! On a hotspot that means everything it serves, including recordings and the
//! device's own identifiers, is readable by anyone who knows the WiFi password.
//! This makes that optional rather than unavoidable.
//!
//! **What this does not do.** There is no TLS on these devices, so credentials
//! cross the air readable by anyone who can already decrypt the WiFi traffic.
//! What it buys is a second factor beyond WiFi access: a guest on the network,
//! or someone who was given the WiFi password for another reason, no longer
//! gets the recordings for free. It is not protection against an attacker
//! positioned to capture the traffic itself.
//!
//! Passwords are stored as PBKDF2-HMAC-SHA256 with a random salt, so a copy of
//! `config.toml`, which is world readable on the device and ends up in support
//! bundles, does not hand over the passwords themselves.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Iterations for the key derivation.
///
/// A compromise for hardware with one slow core: high enough that guessing a
/// stolen hash is expensive, low enough that logging in does not feel broken.
/// Measured on an Orbic RC400L rather than guessed.
pub const ITERATIONS: u32 = 20_000;

/// Largest stored iteration count honoured. The count travels in the hash
/// string, which comes from a world-readable config file, so a tampered or
/// corrupt value could otherwise ask for billions of iterations and hang the
/// login forever. Well above any legitimate setting.
pub const MAX_ITERATIONS: u32 = 1_000_000;

/// Longest username and password accepted, on both the login and the
/// enrollment path. PBKDF2 cost grows with password length, so an unbounded
/// password is a way to make the device spend itself; these are generous but
/// bounded.
pub const MAX_USERNAME_LEN: usize = 64;
pub const MAX_PASSWORD_LEN: usize = 256;

const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;

/// One account permitted to use the web interface.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct WebUser {
    pub username: String,
    /// `pbkdf2-sha256$<iterations>$<salt base64>$<hash base64>`.
    ///
    /// Self describing, so the iteration count can be raised later without
    /// invalidating passwords set before the change.
    pub password_hash: String,
}

/// PBKDF2-HMAC-SHA256, as specified in RFC 8018.
///
/// Written out rather than pulled in as a dependency because it is a short,
/// fully specified construction and the primitives it needs are already here.
/// It is verified against the RFC 6070 test vectors below; a key derivation
/// nobody checked against published vectors is not worth having.
fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    let mut block_index: u32 = 1;
    let mut written = 0;

    while written < out.len() {
        let mut mac = HmacSha256::new_from_slice(password).expect("hmac accepts any key length");
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());
        let mut current = mac.finalize().into_bytes();
        let mut accumulator = current;

        for _ in 1..iterations {
            let mut mac =
                HmacSha256::new_from_slice(password).expect("hmac accepts any key length");
            mac.update(&current);
            current = mac.finalize().into_bytes();
            for (acc, byte) in accumulator.iter_mut().zip(current.iter()) {
                *acc ^= byte;
            }
        }

        let take = (out.len() - written).min(accumulator.len());
        out[written..written + take].copy_from_slice(&accumulator[..take]);
        written += take;
        block_index += 1;
    }
}

/// One PBKDF2 at a time, whoever asks.
///
/// Each verification costs real time on a single slow core, and it runs on
/// the blocking pool, so a burst of guesses could otherwise hold several
/// threads and starve the capture. Guesses are already slowed per attempt
/// by the pairing backoff; this bounds what they can cost in parallel.
static KDF_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Run a key derivation on the blocking pool, one at a time.
pub async fn kdf<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let _permit = KDF_GATE.acquire().await.ok()?;
    tokio::task::spawn_blocking(work).await.ok()
}

/// Hash a password for storage, with a fresh random salt.
pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    // From the OS, like every other secret here. A salt need not be secret,
    // but it must be unpredictable and unrepeated, and the fast generator
    // used before is neither by construction.
    if crate::tls::random_bytes(&mut salt).is_err() {
        for byte in salt.iter_mut() {
            *byte = fastrand::u8(..);
        }
    }
    let mut hash = [0u8; HASH_LEN];
    pbkdf2(password.as_bytes(), &salt, ITERATIONS, &mut hash);
    format!(
        "pbkdf2-sha256${}${}${}",
        ITERATIONS,
        B64.encode(salt),
        B64.encode(hash)
    )
}

/// Whether `password` matches a stored hash.
///
/// Compares in constant time. A comparison that returns early on the first
/// wrong byte leaks how much of a guess was right, which is enough to recover a
/// hash one byte at a time.
pub fn verify_password(password: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    if parts.next() != Some("pbkdf2-sha256") {
        return false;
    }
    let Some(iterations) = parts.next().and_then(|v| v.parse::<u32>().ok()) else {
        return false;
    };
    let Some(salt) = parts.next().and_then(|v| B64.decode(v).ok()) else {
        return false;
    };
    let Some(expected) = parts.next().and_then(|v| B64.decode(v).ok()) else {
        return false;
    };
    if parts.next().is_some()
        || expected.len() != HASH_LEN
        || iterations == 0
        || iterations > MAX_ITERATIONS
    {
        return false;
    }

    let mut actual = [0u8; HASH_LEN];
    pbkdf2(password.as_bytes(), &salt, iterations, &mut actual);

    let mut difference = 0u8;
    for (a, b) in actual.iter().zip(expected.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// A syntactically valid PBKDF2 hash of no useful password, used so an unknown
/// username costs the same PBKDF2 work as a known one. The salt and hash are
/// all-zero; no real password hashes to this, so it never matches by accident.
/// Its structure is checked in the tests.
const DUMMY_HASH: &str =
    "pbkdf2-sha256$20000$AAAAAAAAAAAAAAAAAAAAAA==$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// Whether these credentials match any configured account.
///
/// Exactly one PBKDF2 verification runs whether or not the username exists, so
/// the time taken does not reveal which usernames are real: a known username is
/// verified against its stored hash, an unknown one against a fixed dummy hash
/// and then rejected. Absurd lengths are refused before any hashing, since
/// PBKDF2 cost grows with the password length.
pub fn credentials_are_valid(users: &[WebUser], username: &str, password: &str) -> bool {
    if username.len() > MAX_USERNAME_LEN || password.len() > MAX_PASSWORD_LEN {
        return false;
    }
    let matched = users.iter().find(|u| u.username == username);
    let hash = matched
        .map(|u| u.password_hash.as_str())
        .unwrap_or(DUMMY_HASH);
    // Runs unconditionally; the result is only honoured for a real username.
    let password_ok = verify_password(password, hash);
    matched.is_some() && password_ok
}

/// Decode an HTTP Basic `Authorization` header into a username and password.
pub fn parse_basic_auth(header: &str) -> Option<(String, String)> {
    let encoded = header.strip_prefix("Basic ")?;
    let decoded = B64.decode(encoded.trim()).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    // A password may itself contain a colon, so only the first one separates.
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

/// Which listener a request arrived on. Stamped onto every request as an
/// extension by the listener that accepted it, so the rules below can tell
/// a USB port-forward from the hotspot without asking the socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerKind {
    /// 127.0.0.1: an adb port-forward, or the unit talking to itself. USB
    /// is physical possession, which is already the reset path, so nothing
    /// is required here.
    Loopback,
    /// The plain port on the hotspot. Redirected to TLS while TLS is up.
    Plain,
    /// The TLS port on the hotspot.
    Tls,
}

/// The trusted device a request was made from, once the cookie has checked
/// out. Handlers that need to know which browser is asking read this.
#[derive(Debug, Clone)]
pub struct CurrentDevice(pub String);

/// `/<prefix>/<one segment>` and nothing deeper, so a route added under a
/// pairing prefix later is not open by accident.
fn one_segment_under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .map(|rest| !rest.is_empty() && !rest.contains('/'))
        .unwrap_or(false)
}

/// Paths a browser that is not yet trusted may reach: the pairing page and
/// what it needs, and nothing that reads or changes anything.
pub fn is_exempt_from_pairing(path: &str) -> bool {
    matches!(
        path,
        "/pair"
            | "/api/setup/status"
            | "/api/setup/complete"
            | "/api/pair/passphrase"
            | "/api/pair/account"
            | "/api/pair/code"
            | "/api/setup/press-request"
            | "/api/setup/complete-press"
            | "/api/tls-info"
            | "/api/ca.pem"
            | "/api/ca.crt"
            | "/api/ca.mobileconfig"
            | "/favicon.png"
            | "/rayhunter_orca_only.png"
            | "/rayhunter_text.png"
    ) || one_segment_under(path, "/s/")
        || one_segment_under(path, "/S/")
        || one_segment_under(path, "/p/")
        || one_segment_under(path, "/P/")
        || one_segment_under(path, "/api/setup/press-status/")
}

/// Whether a request is a GPS position being submitted while the operator
/// has opened that one request to unpaired devices.
///
/// Only the submitting side, only by POST, and only while the API GPS mode
/// is on: a switch left on with GPS off must not leave a hole behind.
/// Reading positions back stays behind pairing, since where the unit is
/// belongs to its owner.
pub fn is_open_gps_post(
    config: &crate::config::Config,
    method: &axum::http::Method,
    path: &str,
) -> bool {
    config.gps_api_open
        && config.gps_mode == crate::config::GpsMode::Api
        && *method == axum::http::Method::POST
        && path == "/api/gps"
}

/// The host a link or redirect may point at, from what the request said.
///
/// A `Host` header is whatever the client sent. Reflected into a redirect
/// it would send a browser wherever the sender liked, with the path, token
/// included, along for the ride; reflected into a pairing link it would put
/// a stranger's address on the unit's own screen. So it is honoured only
/// when it is one of the unit's own names: `rayhunter.local`, or an address
/// the unit is serving on. Anything else becomes the hotspot address.
pub async fn canonical_host(state: &crate::server::ServerState, raw_host: Option<&str>) -> String {
    let fallback = state
        .own_addresses
        .iter()
        .find(|ip| !ip.is_loopback())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| crate::tls::LOCAL_NAME.to_string());
    let Some(raw) = raw_host else {
        return fallback;
    };
    let host = host_without_port(raw);
    if host.eq_ignore_ascii_case(crate::tls::LOCAL_NAME) {
        return crate::tls::LOCAL_NAME.to_string();
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if state.own_addresses.contains(&ip) {
            return host.to_string();
        }
        let sta = state
            .wifi_status
            .read()
            .await
            .ip
            .as_deref()
            .and_then(|s| s.parse::<std::net::IpAddr>().ok());
        if sta == Some(ip) {
            return host.to_string();
        }
    }
    fallback
}

/// Whether `host` reaches Rayhunter without a port: `rayhunter.local`, or
/// the front-door alias. Everything else needs the TLS port spelled out.
pub fn is_front_door_host(state: &crate::server::ServerState, host: &str) -> bool {
    host.eq_ignore_ascii_case(crate::tls::LOCAL_NAME)
        || state
            .front_door_alias
            .map(|a| a.to_string() == host)
            .unwrap_or(false)
}

/// Axum middleware adding the browser policy headers every response should
/// carry: no sniffing, no framing, no referrers (a setup link carries its
/// token in the path), and no caching of anything the API says.
pub async fn security_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::HeaderValue;
    let is_api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if is_api && !headers.contains_key("cache-control") {
        headers.insert("cache-control", HeaderValue::from_static("no-store"));
    }
    response
}

fn basic_challenge() -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    // The realm makes browsers prompt rather than showing a bare error.
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"Rayhunter\", charset=\"UTF-8\"",
        )],
        "authentication required",
    )
        .into_response()
}

/// Axum middleware that lets in trusted browsers and nobody else.
///
/// In order: the pairing page and its API are open to all, since they are
/// how a browser becomes trusted; loopback is trusted outright; a valid
/// device cookie passes. Anything else is sent to the pairing page, or told
/// so in JSON if it is an API call. Web accounts from before pairing are no
/// longer a way in on their own: basic auth crossed the hotspot in the
/// clear, which is what pairing exists to end. They can be used once, on
/// the pairing page, to pair the browser.
///
/// If TLS is not up there is no way to pair, so nothing can be required:
/// the unit falls back to exactly what it did before pairing existed, open
/// unless accounts are configured, in which case basic auth is honoured for
/// that run only. A unit whose TLS broke stays usable.
///
/// Applied to every route rather than a chosen list. Guessing which endpoints
/// are sensitive is how one gets forgotten, and the interesting data here is
/// spread across most of them.
pub async fn require_auth(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::server::ServerState>>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Redirect};

    let path = request.uri().path().to_string();
    if is_exempt_from_pairing(&path) {
        return next.run(request).await;
    }
    if is_open_gps_post(&state.config, request.method(), &path) {
        return next.run(request).await;
    }
    let kind = request
        .extensions()
        .get::<ListenerKind>()
        .copied()
        .unwrap_or(ListenerKind::Plain);
    if kind == ListenerKind::Loopback {
        return next.run(request).await;
    }

    let cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| crate::pairing::cookie_value(h, crate::pairing::COOKIE_NAME))
        .map(str::to_string);
    if let Some(token) = cookie
        && let Some(device) = state.pairing.authenticate(&token).await
    {
        request.extensions_mut().insert(CurrentDevice(device.id));
        return next.run(request).await;
    }

    if state.tls.is_none() {
        // The live list, so an account set a moment ago is already in force
        // and one just deleted has already stopped working.
        let users = state.web_users.read().await.clone();
        if users.is_empty() {
            return next.run(request).await;
        }
        let supplied = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_basic_auth);
        if let Some((username, password)) = supplied {
            let valid = kdf(move || credentials_are_valid(&users, &username, &password))
                .await
                .unwrap_or(false);
            if valid {
                return next.run(request).await;
            }
        }
        return basic_challenge();
    }

    if path.starts_with("/api/") {
        (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"this browser is not paired with the unit","pair":"/pair"}"#,
        )
            .into_response()
    } else {
        Redirect::to("/pair").into_response()
    }
}

/// Whether a `Host` header names a port.
fn host_had_port(host: &str) -> bool {
    match host.strip_prefix('[') {
        // An IPv6 literal: a port can only follow the closing bracket.
        Some(rest) => rest
            .split_once(']')
            .map(|(_, after)| after.starts_with(':'))
            .unwrap_or(false),
        None => host.contains(':'),
    }
}

/// The host part of a `Host` header, without the port.
fn host_without_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        // An IPv6 literal: everything up to the closing bracket.
        return rest.split_once(']').map(|(h, _)| h).unwrap_or(rest);
    }
    host.split_once(':').map(|(h, _)| h).unwrap_or(host)
}

/// Axum middleware sending the plain hotspot port to the TLS one.
///
/// Only the plain listener on the hotspot, and only while TLS is up. The
/// loopback port keeps serving as it always has, and a unit whose TLS is
/// down keeps its plain port rather than pointing at nothing. One line of
/// text goes with the redirect for a client that cannot follow it.
pub async fn redirect_to_tls(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::server::ServerState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;

    let kind = request
        .extensions()
        .get::<ListenerKind>()
        .copied()
        .unwrap_or(ListenerKind::Plain);
    if kind != ListenerKind::Plain || state.tls.is_none() {
        return next.run(request).await;
    }
    // A GPS app posting positions cannot follow a redirect to a certificate
    // it does not trust, so the one request the operator opened is served
    // where it arrived.
    if is_open_gps_post(&state.config, request.method(), request.uri().path()) {
        return next.run(request).await;
    }
    let raw_host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| !h.is_empty())
        .map(str::to_string);
    let raw_host = raw_host.as_deref().unwrap_or("");
    // Only the unit's own names are reflected; anything else goes to the
    // hotspot address, with the port that implies.
    let checked = canonical_host(&state, Some(raw_host)).await;
    let reflected = host_without_port(raw_host).eq_ignore_ascii_case(&checked);
    let host = checked.as_str();
    // A request that arrived with no port at one of the two front-door
    // names came in on port 80, where 443 is Rayhunter's too; keep it
    // portless. Anything else, including a host that had to be replaced,
    // names the TLS port: on the hotspot address itself, 443 is the
    // hotspot's admin page, not Rayhunter.
    let had_port = !reflected || host_had_port(raw_host) || !is_front_door_host(&state, host);
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let rest = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let target = if had_port {
        format!("https://{host}:{}{rest}", state.config.tls_port)
    } else {
        format!("https://{host}{rest}")
    };
    // Temporary and method-preserving: the right address can change with
    // the network, and a permanent answer would be cached past that.
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, target.clone())],
        format!("Rayhunter is served over HTTPS now: {target}\n"),
    )
        .into_response()
}

/// Whether a request is a state-changing one made by some other website.
///
/// Two independent signals a browser attaches, checked so that either one
/// catching a forgery is enough:
///
/// - `Origin`: on a state-changing request a browser sends the origin of the
///   page that made it. If that origin's authority does not match the device's
///   own `Host`, the request came from another site. Origin is sent by every
///   current browser for these methods, including same-origin ones, so it is
///   the primary check.
/// - `Sec-Fetch-Site`: a request the page made to its own origin is
///   `same-origin`; one another site triggered is `cross-site`. Used as a
///   fallback for the rare browser that sends no `Origin`.
///
/// A request with neither header is not from a browser (curl, the dev proxy, an
/// app) and so is not a cross-site vector, and is allowed. Only state-changing
/// methods are considered; the browser's same-origin policy already stops
/// another site reading the response to a GET.
fn is_cross_site_state_change(
    method: &axum::http::Method,
    sec_fetch_site: Option<&str>,
    origin: Option<&str>,
    host: Option<&str>,
) -> bool {
    use axum::http::Method;
    let mutating = matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if !mutating {
        return false;
    }
    if let Some(origin) = origin {
        // Compare the authority (host[:port]) of the Origin to our Host. The
        // Origin carries a scheme ("http://host:port"); the Host does not.
        let origin_authority = origin.split_once("://").map(|(_, rest)| rest);
        return match (origin_authority, host) {
            (Some(o), Some(h)) if o == h => false,
            // Origin present but not our own (or nothing to compare it to).
            _ => true,
        };
    }
    match sec_fetch_site {
        // The app talking to itself, or a user-initiated navigation.
        Some("same-origin") | Some("same-site") | Some("none") | None => false,
        // "cross-site", and anything unrecognised, is treated as hostile.
        Some(_) => true,
    }
}

/// Axum middleware that refuses cross-site state-changing requests.
///
/// This stops a web page you happen to visit from silently deleting your
/// recordings or rewriting the configuration on a Rayhunter it can reach over
/// the network — a cross-site request forgery. It is independent of the
/// password: it protects the device whether or not an account is configured,
/// and the legitimate web UI, which only ever calls the API from its own
/// origin, is unaffected.
pub async fn csrf_protection(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let headers = request.headers();
    let sec_fetch_site = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    if is_cross_site_state_change(request.method(), sec_fetch_site, origin, host) {
        return (
            StatusCode::FORBIDDEN,
            "cross-site request refused; this action can only be taken from the Rayhunter interface itself",
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checked against RFC 6070, which publishes PBKDF2 vectors for HMAC-SHA1,
    /// and against the widely reproduced HMAC-SHA256 equivalents. A key
    /// derivation nobody checked against published vectors is not worth having,
    /// because it will still look like it works.
    #[test]
    fn matches_the_published_test_vectors() {
        let mut out = [0u8; 32];
        pbkdf2(b"password", b"salt", 1, &mut out);
        assert_eq!(
            hex(&out),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );

        let mut out = [0u8; 32];
        pbkdf2(b"password", b"salt", 2, &mut out);
        assert_eq!(
            hex(&out),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );

        let mut out = [0u8; 32];
        pbkdf2(b"password", b"salt", 4096, &mut out);
        assert_eq!(
            hex(&out),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_password_verifies_against_its_own_hash() {
        let stored = hash_password("correct horse battery staple");
        assert!(verify_password("correct horse battery staple", &stored));
        assert!(!verify_password("Correct horse battery staple", &stored));
        assert!(!verify_password("", &stored));
    }

    /// Two hashes of the same password must differ, or a stolen config reveals
    /// which accounts share a password.
    #[test]
    fn the_same_password_hashes_differently_each_time() {
        let a = hash_password("hunter2");
        let b = hash_password("hunter2");
        assert_ne!(a, b, "salt is not being applied");
        assert!(verify_password("hunter2", &a));
        assert!(verify_password("hunter2", &b));
    }

    /// A malformed or truncated hash must fail closed. Returning true for
    /// something unparseable would turn a corrupt config into an open door.
    #[test]
    fn a_malformed_hash_never_verifies() {
        for stored in [
            "",
            "not-a-hash",
            "pbkdf2-sha256$",
            "pbkdf2-sha256$0$c2FsdA==$aGFzaA==",
            "md5$1$c2FsdA==$aGFzaA==",
            "pbkdf2-sha256$1000$notbase64!$notbase64!",
            "pbkdf2-sha256$1000$c2FsdA==$c2hvcnQ=",
        ] {
            assert!(!verify_password("anything", stored), "accepted {stored:?}");
        }
    }

    #[test]
    fn credentials_are_checked_against_every_account() {
        let users = vec![
            WebUser {
                username: "alice".into(),
                password_hash: hash_password("alpha"),
            },
            WebUser {
                username: "bob".into(),
                password_hash: hash_password("bravo"),
            },
        ];
        assert!(credentials_are_valid(&users, "alice", "alpha"));
        assert!(credentials_are_valid(&users, "bob", "bravo"));
        assert!(!credentials_are_valid(&users, "alice", "bravo"));
        assert!(!credentials_are_valid(&users, "carol", "alpha"));
        assert!(!credentials_are_valid(&[], "alice", "alpha"));
    }

    /// The dummy hash must be a well-formed PBKDF2 string, or an unknown
    /// username would fail parsing early and skip the work that makes its
    /// timing match a known one. Parse it exactly as verify_password does.
    #[test]
    fn the_dummy_hash_is_well_formed() {
        let mut parts = DUMMY_HASH.split('$');
        assert_eq!(parts.next(), Some("pbkdf2-sha256"));
        let iterations: u32 = parts.next().unwrap().parse().unwrap();
        assert!(iterations > 0 && iterations <= MAX_ITERATIONS);
        let salt = B64.decode(parts.next().unwrap()).unwrap();
        let expected = B64.decode(parts.next().unwrap()).unwrap();
        assert!(parts.next().is_none());
        assert_eq!(salt.len(), SALT_LEN);
        assert_eq!(expected.len(), HASH_LEN);
        // And it never matches a real attempt.
        assert!(!verify_password("anything", DUMMY_HASH));
    }

    /// Absurdly long credentials are refused before any hashing, so they cannot
    /// be used to make the device grind. Enforced on the login path itself.
    #[test]
    fn overlong_credentials_are_refused() {
        let users = vec![WebUser {
            username: "alice".into(),
            password_hash: hash_password("alpha"),
        }];
        let huge = "a".repeat(MAX_PASSWORD_LEN + 1);
        assert!(!credentials_are_valid(&users, "alice", &huge));
        let long_name = "a".repeat(MAX_USERNAME_LEN + 1);
        assert!(!credentials_are_valid(&users, &long_name, "alpha"));
    }

    /// A hash whose stored iteration count is absurd is rejected rather than
    /// honoured, so a tampered config cannot ask for billions of iterations.
    #[test]
    fn an_absurd_iteration_count_is_refused() {
        let stored = format!(
            "pbkdf2-sha256${}$AAAAAAAAAAAAAAAAAAAAAA==$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            MAX_ITERATIONS + 1
        );
        assert!(!verify_password("anything", &stored));
    }

    #[test]
    fn basic_auth_headers_decode() {
        // "alice:alpha"
        assert_eq!(
            parse_basic_auth("Basic YWxpY2U6YWxwaGE="),
            Some(("alice".into(), "alpha".into()))
        );
    }

    /// A colon is legal inside a password and only the first one separates.
    #[test]
    fn a_password_may_contain_a_colon() {
        let encoded = B64.encode("alice:pass:with:colons");
        assert_eq!(
            parse_basic_auth(&format!("Basic {encoded}")),
            Some(("alice".into(), "pass:with:colons".into()))
        );
    }

    #[test]
    fn cross_site_state_changes_are_refused_but_the_ui_and_reads_are_not() {
        use axum::http::Method;
        let host = Some("192.168.1.1:8080");

        // The legitimate web UI: a same-origin Origin matching our Host.
        assert!(!is_cross_site_state_change(
            &Method::POST,
            Some("same-origin"),
            Some("http://192.168.1.1:8080"),
            host,
        ));
        // A malicious page: its Origin is some other site.
        assert!(is_cross_site_state_change(
            &Method::POST,
            None,
            Some("https://evil.example"),
            host,
        ));
        // Origin absent: fall back to Sec-Fetch-Site.
        assert!(is_cross_site_state_change(
            &Method::POST,
            Some("cross-site"),
            None,
            host
        ));
        assert!(is_cross_site_state_change(
            &Method::DELETE,
            Some("cross-site"),
            None,
            host
        ));
        assert!(!is_cross_site_state_change(
            &Method::POST,
            Some("same-site"),
            None,
            host
        ));
        // No browser headers at all (curl, the dev proxy): allowed.
        assert!(!is_cross_site_state_change(&Method::POST, None, None, host));
        assert!(!is_cross_site_state_change(
            &Method::POST,
            Some("none"),
            None,
            host
        ));
        // Reads are never blocked, whatever their origin.
        assert!(!is_cross_site_state_change(
            &Method::GET,
            Some("cross-site"),
            Some("https://evil.example"),
            host,
        ));
        assert!(!is_cross_site_state_change(
            &Method::HEAD,
            Some("cross-site"),
            None,
            host
        ));
    }

    #[test]
    fn the_pairing_page_and_its_api_are_open_and_nothing_else_is() {
        for open in [
            "/pair",
            "/s/7K3M9XWQ",
            "/S/7K3M9XWQ",
            "/api/setup/status",
            "/api/setup/complete",
            "/api/pair/passphrase",
            "/api/pair/code",
            "/p/123456",
            "/api/setup/press-request",
            "/api/setup/press-status/abc",
            "/api/tls-info",
            "/favicon.png",
        ] {
            assert!(is_exempt_from_pairing(open), "{open} should be open");
        }
        // Only one segment under a pairing prefix; nothing deeper, nothing
        // empty, so a later route under the prefix is not open by accident.
        for closed in [
            "/s/",
            "/s/abc/def",
            "/p/123456/extra",
            "/api/setup/press-status/",
        ] {
            assert!(
                !is_exempt_from_pairing(closed),
                "{closed} should need pairing"
            );
        }
        for closed in [
            "/",
            "/index.html",
            "/api/config",
            "/api/qmdl-manifest",
            "/api/devices",
            "/api/terminal",
            "/api/setup",
            "/spair",
            "/api/devices/code",
            "/api/stepup/start",
            "/api/passphrase",
        ] {
            assert!(
                !is_exempt_from_pairing(closed),
                "{closed} should need pairing"
            );
        }
    }

    #[test]
    fn the_gps_submission_opens_only_when_asked_and_only_for_posting() {
        use crate::config::{Config, GpsMode};
        use axum::http::Method;
        let mut config = Config {
            gps_mode: GpsMode::Api,
            ..Config::default()
        };
        // Off by default, whatever the mode.
        assert!(!is_open_gps_post(&config, &Method::POST, "/api/gps"));
        config.gps_api_open = true;
        assert!(is_open_gps_post(&config, &Method::POST, "/api/gps"));
        // Reading positions back stays paired-only.
        assert!(!is_open_gps_post(&config, &Method::GET, "/api/gps"));
        // Nothing else under the API opens with it.
        assert!(!is_open_gps_post(&config, &Method::POST, "/api/gps/"));
        assert!(!is_open_gps_post(&config, &Method::POST, "/api/config"));
        // A switch left on with the API mode off leaves no hole behind.
        config.gps_mode = GpsMode::Fixed;
        assert!(!is_open_gps_post(&config, &Method::POST, "/api/gps"));
        config.gps_mode = GpsMode::Disabled;
        assert!(!is_open_gps_post(&config, &Method::POST, "/api/gps"));
    }

    #[test]
    fn a_portless_host_means_the_front_door() {
        assert!(host_had_port("192.168.1.1:8080"));
        assert!(!host_had_port("rayhunter.local"));
        assert!(!host_had_port("192.168.1.254"));
        assert!(host_had_port("[::1]:8080"));
        assert!(!host_had_port("[::1]"));
    }

    #[test]
    fn the_host_loses_its_port_and_keeps_its_brackets() {
        assert_eq!(host_without_port("192.168.1.1:8080"), "192.168.1.1");
        assert_eq!(host_without_port("192.168.1.1"), "192.168.1.1");
        assert_eq!(host_without_port("rayhunter.local:8080"), "rayhunter.local");
        assert_eq!(host_without_port("[::1]:8080"), "::1");
    }

    #[test]
    fn rubbish_headers_are_refused() {
        assert_eq!(parse_basic_auth(""), None);
        assert_eq!(parse_basic_auth("Bearer abc"), None);
        assert_eq!(parse_basic_auth("Basic !!!not base64!!!"), None);
        // Valid base64 with no colon is not a credential pair.
        assert_eq!(
            parse_basic_auth(&format!("Basic {}", B64.encode("nocolon"))),
            None
        );
    }
}
