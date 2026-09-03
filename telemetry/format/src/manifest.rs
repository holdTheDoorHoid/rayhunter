//! What a submission says about itself, and the service's public description
//! of itself.
//!
//! The manifest is sent as the body of the request that opens a submission,
//! with its signature in a header, and the server keeps the exact bytes it
//! received. Signing the bytes as sent, rather than some canonical form,
//! means neither side has to agree on how JSON is laid out.

use serde::{Deserialize, Serialize};

use crate::FORMAT;

/// Which of the two bundles a submission carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum Tier {
    /// The shareable bundle: identifiers zeroed, raw capture left out.
    Summary,
    /// The summary bundle plus the raw capture, encrypted to the archive key.
    Full,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Summary => "summary",
            Tier::Full => "full",
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What each encrypted part of a submission is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// The summary bundle, encrypted to the ingest key.
    Summary,
    /// The raw capture bundle, encrypted to the archive key.
    Capture,
}

/// One encrypted file in a submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartInfo {
    /// The file name used in the upload URL, for example `summary.enc`.
    pub name: String,
    pub kind: PartKind,
    /// Which service key it was encrypted to, by id.
    pub recipient_key_id: String,
    pub plaintext_bytes: u64,
    pub ciphertext_bytes: u64,
    /// SHA-256 of the ciphertext, hex. The server checks it as the part
    /// arrives.
    pub sha256: String,
}

/// The owner's choice, carried with every submission so it can be shown
/// later that the data was sent with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct Consent {
    pub tier: Tier,
    /// When the owner acknowledged what the full tier contains. Required for
    /// a full submission; absent for a summary one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<String>,
}

/// The software that made the submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// The signed document that opens a submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub submission_id: String,
    /// RFC 3339, the unit's corrected clock.
    pub created_at: String,
    pub tier: Tier,
    /// The unit's signing key, base64 SEC1, that the signature header is
    /// checked against.
    pub submitter_public_key: String,
    pub consent: Consent,
    pub client: ClientInfo,
    pub parts: Vec<PartInfo>,
}

impl Manifest {
    /// Checks that do not need the server's policy: the format, the id, and
    /// that the parts match the tier.
    pub fn check_shape(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("unknown format {:?}", self.format));
        }
        if !crate::is_submission_id(&self.submission_id) {
            return Err("malformed submission id".into());
        }
        if self.consent.tier != self.tier {
            return Err("the consent record names a different tier".into());
        }
        if self.tier == Tier::Full && self.consent.acknowledged_at.is_none() {
            return Err("a full submission must carry the owner's acknowledgement".into());
        }
        let summaries = self
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::Summary)
            .count();
        let captures = self
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::Capture)
            .count();
        if summaries != 1 {
            return Err("a submission carries exactly one summary part".into());
        }
        match self.tier {
            Tier::Summary if captures != 0 => {
                return Err("a summary submission carries no capture part".into());
            }
            Tier::Full if captures != 1 => {
                return Err("a full submission carries exactly one capture part".into());
            }
            _ => {}
        }
        for part in &self.parts {
            if !is_part_name(&part.name) {
                return Err(format!("part name {:?} is not allowed", part.name));
            }
            if part.sha256.len() != 64 || !part.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("part {} has a malformed hash", part.name));
            }
        }
        let mut names: Vec<&str> = self.parts.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        if names.len() != self.parts.len() {
            return Err("two parts share a name".into());
        }
        Ok(())
    }
}

/// A part name is a plain file name: letters, digits, dot, dash and
/// underscore, not starting with a dot. It becomes a path segment on the
/// server.
pub fn is_part_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
}

/// The service's description of itself, served at
/// `/.well-known/rayhunter-telemetry`. A unit fetches it once, shows the key
/// fingerprints to its owner, and pins the keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ServerInfo {
    pub format: String,
    /// A name for people, for example "Example Community Rayhunter Dataset".
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How to reach whoever runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    /// Where the collected data is published, if anywhere yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    /// Base64 SEC1. Summary parts are encrypted to this.
    pub ingest_public_key: String,
    /// Base64 SEC1. Capture parts are encrypted to this. Absent when the
    /// service does not accept full submissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_public_key: Option<String>,
    pub accepted_tiers: Vec<Tier>,
    pub max_summary_bytes: u64,
    pub max_capture_bytes: u64,
}

/// A signed request to remove a submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithdrawRequest {
    pub format: String,
    pub submission_id: String,
    pub requested_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The header carrying a request body's signature.
pub const SIGNATURE_HEADER: &str = "x-rayhunter-signature";

/// The path of the service description.
pub const WELL_KNOWN_PATH: &str = "/.well-known/rayhunter-telemetry";

/// What the server signs its "finalize" check against: the body of a
/// finalize request is this string, so a replayed manifest signature cannot
/// finalize.
pub fn finalize_message(submission_id: &str) -> Vec<u8> {
    format!("{FORMAT}|finalize|{submission_id}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: PartKind, name: &str) -> PartInfo {
        PartInfo {
            name: name.into(),
            kind,
            recipient_key_id: "0000000000000000".into(),
            plaintext_bytes: 1,
            ciphertext_bytes: 2,
            sha256: "0".repeat(64),
        }
    }

    fn manifest(tier: Tier, parts: Vec<PartInfo>) -> Manifest {
        Manifest {
            format: FORMAT.into(),
            submission_id: "0123456789abcdef0123456789abcdef".into(),
            created_at: "2026-09-02T12:00:00Z".into(),
            tier,
            submitter_public_key: "AAAA".into(),
            consent: Consent {
                tier,
                acknowledged_at: (tier == Tier::Full).then(|| "2026-09-01T00:00:00Z".into()),
            },
            client: ClientInfo {
                name: "rayhunter".into(),
                version: "0.12.3".into(),
            },
            parts,
        }
    }

    #[test]
    fn well_formed_manifests_pass() {
        manifest(Tier::Summary, vec![part(PartKind::Summary, "summary.enc")])
            .check_shape()
            .unwrap();
        manifest(
            Tier::Full,
            vec![
                part(PartKind::Summary, "summary.enc"),
                part(PartKind::Capture, "capture.enc"),
            ],
        )
        .check_shape()
        .unwrap();
    }

    /// The shape checks are the server's first line against a malformed or
    /// mischievous manifest, so each rule has a case.
    #[test]
    fn malformed_manifests_are_named_specifically() {
        let m = manifest(Tier::Summary, vec![part(PartKind::Summary, "summary.enc")]);

        let mut bad = m.clone();
        bad.format = "something/2".into();
        assert!(bad.check_shape().unwrap_err().contains("format"));

        let mut bad = m.clone();
        bad.submission_id = "../../x".into();
        assert!(bad.check_shape().unwrap_err().contains("submission id"));

        let mut bad = m.clone();
        bad.parts.push(part(PartKind::Capture, "capture.enc"));
        assert!(bad.check_shape().unwrap_err().contains("no capture"));

        let mut bad = manifest(
            Tier::Full,
            vec![
                part(PartKind::Summary, "summary.enc"),
                part(PartKind::Capture, "capture.enc"),
            ],
        );
        bad.consent.acknowledged_at = None;
        assert!(bad.check_shape().unwrap_err().contains("acknowledgement"));

        let mut bad = m.clone();
        bad.consent.tier = Tier::Full;
        assert!(bad.check_shape().unwrap_err().contains("consent"));

        let mut bad = m.clone();
        bad.parts[0].name = "../summary.enc".into();
        assert!(bad.check_shape().unwrap_err().contains("not allowed"));

        let mut bad = m.clone();
        bad.parts[0].sha256 = "zz".into();
        assert!(bad.check_shape().unwrap_err().contains("hash"));

        let mut bad = m.clone();
        bad.parts.clear();
        assert!(
            bad.check_shape()
                .unwrap_err()
                .contains("exactly one summary")
        );

        let mut bad = manifest(Tier::Full, vec![part(PartKind::Summary, "a.enc")]);
        bad.parts.push(part(PartKind::Capture, "a.enc"));
        assert!(bad.check_shape().unwrap_err().contains("share a name"));
    }

    #[test]
    fn part_names_are_plain_file_names() {
        for ok in ["summary.enc", "capture.enc", "a-b_c.1"] {
            assert!(is_part_name(ok), "{ok}");
        }
        for bad in [
            "",
            ".hidden",
            "../x",
            "a/b",
            "a b",
            "\u{e9}",
            &"x".repeat(65),
        ] {
            assert!(!is_part_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn documents_round_trip_through_json() {
        let m = manifest(
            Tier::Full,
            vec![
                part(PartKind::Summary, "summary.enc"),
                part(PartKind::Capture, "capture.enc"),
            ],
        );
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"tier\":\"full\""));
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);

        let info = ServerInfo {
            format: FORMAT.into(),
            name: "Test".into(),
            description: None,
            contact: None,
            site_url: None,
            ingest_public_key: "AAAA".into(),
            archive_public_key: None,
            accepted_tiers: vec![Tier::Summary],
            max_summary_bytes: 1,
            max_capture_bytes: 0,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("archive_public_key"));
        let back: ServerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }
}
