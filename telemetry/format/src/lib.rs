//! The envelope around a recording a Rayhunter unit contributes to a
//! community dataset.
//!
//! Two sides share this crate: the daemon, which builds and seals a
//! submission on the device, and `rayhunter-collector`, which receives,
//! verifies and opens it. Keeping the format in one place is what makes the
//! two agree, and what lets the format be tested as a round trip without
//! either of them.
//!
//! Three things live here:
//!
//! - [`keys`]: the service's two recipient keys (ingest and archive) and the
//!   unit's own signing key, with a stable way of naming a key.
//! - [`stream`]: encryption of a file of any size to a recipient key, in
//!   chunks, so a capture of a hundred megabytes never has to sit in memory
//!   on a device with a hundred and sixty.
//! - [`manifest`] and [`summary`]: what a submission says about itself, and
//!   what the summary bundle says about the recording, as the types both
//!   sides serialise.
//!
//! See `telemetry/DESIGN.md` for why the format is shaped this way.

pub mod keys;
pub mod manifest;
pub mod stream;
pub mod summary;

/// The format version every document and stream carries. Bumped when the
/// shape of anything here changes incompatibly.
pub const FORMAT: &str = "rayhunter-telemetry/1";

/// Errors from any part of the format.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("encryption: {0}")]
    Hpke(String),
    #[error("key: {0}")]
    Key(String),
    #[error("the signature does not match")]
    BadSignature,
    #[error("not an encrypted telemetry stream")]
    BadHeader,
    #[error("the stream ended before its final chunk")]
    Truncated,
    #[error("data follows the final chunk")]
    TrailingData,
    #[error("a chunk claims {0} bytes, more than the format allows")]
    ChunkTooLarge(u32),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// A fresh submission identifier: 16 random bytes as 32 hex characters.
///
/// Random rather than derived from anything about the recording, so the id
/// says nothing about when or where it was made.
pub fn new_submission_id() -> String {
    let mut bytes = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    hex(&bytes)
}

/// Lowercase hex of any bytes.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Whether a string is a well-formed submission id, as checked by the
/// server before it becomes a directory name.
pub fn is_submission_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_ids_are_random_hex_of_the_right_length() {
        let a = new_submission_id();
        let b = new_submission_id();
        assert_ne!(a, b);
        assert!(is_submission_id(&a));
        assert!(!is_submission_id("../etc/passwd"));
        assert!(!is_submission_id(&a.to_uppercase()));
        assert!(!is_submission_id(&a[..31]));
    }
}
