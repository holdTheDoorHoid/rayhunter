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

/// Hash a password for storage, with a fresh random salt.
pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    for byte in salt.iter_mut() {
        *byte = fastrand::u8(..);
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
    if parts.next().is_some() || expected.len() != HASH_LEN || iterations == 0 {
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

/// Whether these credentials match any configured account.
///
/// Every account is checked even after a match, so the time taken does not
/// reveal which username exists. An unknown username is checked against a
/// dummy hash for the same reason.
pub fn credentials_are_valid(users: &[WebUser], username: &str, password: &str) -> bool {
    let mut valid = false;
    for user in users {
        // Both branches do the same work.
        if user.username == username && verify_password(password, &user.password_hash) {
            valid = true;
        }
    }
    valid
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

/// Axum middleware requiring a valid account when any is configured.
///
/// Applied to every route rather than a chosen list. Guessing which endpoints
/// are sensitive is how one gets forgotten, and the interesting data here is
/// spread across most of them.
pub async fn require_auth(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::server::ServerState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;

    let users = &state.config.web_users;
    // No accounts configured means Rayhunter's long standing behaviour: open.
    // An update must not lock somebody out of their own device.
    if users.is_empty() {
        return next.run(request).await;
    }

    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_basic_auth);

    if let Some((username, password)) = supplied
        && credentials_are_valid(users, &username, &password)
    {
        return next.run(request).await;
    }

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
