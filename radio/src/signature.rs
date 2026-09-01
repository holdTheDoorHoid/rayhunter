//! Data-driven surveillance signatures.
//!
//! Vendors are never hard-coded into the scanner. A signature is a row of
//! data: what to match, how strongly a match should be believed, and what
//! evidence justifies it. Adding a newly-researched device is a data change.
//!
//! Two properties matter more than breadth of coverage:
//!
//! * **An OUI match alone is weak evidence.** MAC randomisation and shared
//!   OEM hardware mean a vendor prefix says "this could be" and never "this
//!   is". Such rules carry [`Confidence::Info`] and must be presented as such.
//! * **Weak matches must never suppress strong ones.** A device seen first by
//!   its OUI and later by a full behavioural fingerprint has to be promoted,
//!   not deduplicated away. See [`DetectionLog`].

use crate::mac::{MacAddr, MacPrefix};
use crate::observation::{FrameKind, ObservationPayload, RadioTech, WifiObservation};
use serde::{Deserialize, Serialize};

/// Schema version of the signature file format. Bumped when the meaning of an
/// existing field changes, so an old pack is rejected rather than
/// misinterpreted.
pub const SCHEMA_VERSION: u32 = 1;

/// How strongly a match should be believed.
///
/// The ordering is meaningful: it is what allows a later, better-evidenced
/// detection to replace an earlier weak one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// A single weak indicator, typically a vendor OUI. Corroborating only.
    Info,
    /// Several compatible indicators, none decisive on its own.
    Low,
    /// A specific behavioural or structural fingerprint.
    Medium,
    /// Multiple independent signals forming a specific device fingerprint.
    High,
}

impl Confidence {
    pub const fn label(&self) -> &'static str {
        match self {
            Confidence::Info => "INFO",
            Confidence::Low => "LOW",
            Confidence::Medium => "MEDIUM",
            Confidence::High => "HIGH",
        }
    }
}

/// How loudly a detection should be surfaced. Deliberately separate from
/// [`Confidence`]: a high-confidence identification of a benign device should
/// not raise a severe alert, and the threshold at which the device display
/// changes is a policy decision, not a detector one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Informational,
    Low,
    Medium,
    High,
}

/// Which address in an observation a MAC rule applies to.
///
/// Naming the field matters: matching a signature against `addr1` or `addr3`
/// catches access points *answering* a target device rather than the device
/// itself, which is a documented source of false positives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacField {
    /// Any address present in the observation.
    Any,
    Bssid,
    /// 802.11 addr2 — the device that actually transmitted the frame.
    Transmitter,
    /// 802.11 addr1 — the intended receiver. Second-hand evidence.
    Receiver,
    /// 802.11 addr3.
    Addr3,
}

/// One condition within a signature. A signature matches when *all* of its
/// conditions match, which is how composite fingerprints are expressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatchCondition {
    /// An exact address.
    MacExact { field: MacField, mac: MacAddr },
    /// A prefix of any nibble length, including the non-byte-aligned
    /// allocations used by some IMSI-catcher vendors.
    MacPrefix {
        field: MacField,
        prefix: MacPrefix,
        /// Whether to match addresses with the locally-administered bit set.
        ///
        /// Off by default: such an address is normally synthesised by MAC
        /// randomisation and its leading bytes say nothing about the hardware,
        /// so applying a vendor prefix to one invents evidence.
        ///
        /// It is not always right, though. Some deployed surveillance hardware
        /// transmits from a fixed locally-administered address — flock-you
        /// documents `82:6b:f2` as one such camera prefix, and warns that a
        /// blanket randomisation filter silently drops it. A signature that
        /// targets a known fixed prefix of that kind sets this, and should say
        /// why in its `notes`.
        #[serde(default)]
        allow_locally_administered: bool,
    },
    /// The SSID equals this string exactly.
    SsidExact { ssid: String },
    /// The SSID contains this substring.
    SsidContains { substring: String },
    /// The SSID matches a glob (`*` for any run, `?` for one character).
    ///
    /// Bounded by construction — see [`crate::glob`]. This exists because
    /// user-written rules need it; curated signatures should prefer an exact
    /// or prefix match, which is easier to reason about.
    SsidGlob { pattern: String },
    /// A zero-length SSID element: a wildcard probe.
    SsidWildcard,
    /// The observation came from this kind of frame.
    FrameType { frame: FrameKind },
    /// An information element with this ID is present, and — when `prefix` is
    /// given — its payload starts with those bytes.
    InformationElement {
        id: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<Vec<u8>>,
    },
    /// A vendor-specific element carrying this OUI is present.
    VendorOui { oui: MacPrefix },
    /// The exact ordered set of element IDs present in the frame. This is the
    /// "IE fingerprint" of the probe-request literature: which elements a
    /// device includes, and in what order, is characteristic of its firmware.
    IeIdSequence { ids: Vec<u8> },
    /// A Bluetooth SIG company identifier in manufacturer data.
    BleCompanyId { id: u16 },
    /// A BLE service UUID, compared case-insensitively.
    BleServiceUuid { uuid: String },
    /// The BLE local name contains this substring.
    BleNameContains { substring: String },
}

/// A curated or user-supplied detection rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signature {
    /// Stable identifier, referenced by evidence records so a detection stays
    /// interpretable after the rule is edited.
    pub id: String,
    pub vendor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    pub technology: RadioTech,
    pub conditions: Vec<MatchCondition>,
    pub confidence: Confidence,
    pub severity: Severity,
    pub description: String,
    /// Where this signature came from, so a claim can be audited later.
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Signatures whose provenance has not been independently checked ship
    /// disabled, so breadth of coverage never costs alert quality by default.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

const fn default_true() -> bool {
    true
}

/// A versioned collection of signatures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignatureDb {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_version: Option<String>,
    pub signatures: Vec<Signature>,
}

impl SignatureDb {
    pub fn empty() -> Self {
        SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: Vec::new(),
        }
    }

    /// Parse a signature pack, rejecting a schema this build does not
    /// understand rather than silently ignoring fields.
    pub fn from_json(input: &str) -> Result<Self, SignatureError> {
        let db: SignatureDb = serde_json::from_str(input)?;
        if db.schema_version != SCHEMA_VERSION {
            return Err(SignatureError::UnsupportedSchema {
                found: db.schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        db.validate()?;
        Ok(db)
    }

    /// Reject structurally valid but meaningless packs: duplicate IDs make
    /// evidence ambiguous, and a signature with no conditions would match
    /// every device on the air.
    pub fn validate(&self) -> Result<(), SignatureError> {
        let mut seen: Vec<&str> = Vec::with_capacity(self.signatures.len());
        for sig in &self.signatures {
            if sig.id.trim().is_empty() {
                return Err(SignatureError::EmptyId);
            }
            if sig.conditions.is_empty() {
                return Err(SignatureError::NoConditions { id: sig.id.clone() });
            }
            if seen.contains(&sig.id.as_str()) {
                return Err(SignatureError::DuplicateId { id: sig.id.clone() });
            }
            seen.push(&sig.id);
        }
        Ok(())
    }

    /// Every enabled signature that matches, with the reasons it matched.
    pub fn match_observation(&self, payload: &ObservationPayload) -> Vec<Detection> {
        let mut out = Vec::new();
        for sig in self.signatures.iter().filter(|s| s.enabled) {
            if sig.technology != payload.tech() {
                continue;
            }
            if let Some(reasons) = match_all(&sig.conditions, payload) {
                out.push(Detection {
                    signature_id: sig.id.clone(),
                    vendor: sig.vendor.clone(),
                    product: sig.product.clone(),
                    technology: sig.technology,
                    confidence: sig.confidence,
                    severity: sig.severity,
                    matched_fields: reasons,
                });
            }
        }
        out
    }
}

/// A signature that fired, carrying the evidence a user needs to judge it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub signature_id: String,
    pub vendor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    pub technology: RadioTech,
    pub confidence: Confidence,
    pub severity: Severity,
    /// Human-readable reasons, one per satisfied condition. Surfaced verbatim
    /// in the UI so a user is never asked to trust an unexplained verdict.
    pub matched_fields: Vec<String>,
}

/// Returns the reasons every condition matched, or `None` if any did not.
fn match_all(conditions: &[MatchCondition], payload: &ObservationPayload) -> Option<Vec<String>> {
    let mut reasons = Vec::with_capacity(conditions.len());
    for condition in conditions {
        let reason = match_one(condition, payload)?;
        reasons.push(reason);
    }
    Some(reasons)
}

fn match_one(condition: &MatchCondition, payload: &ObservationPayload) -> Option<String> {
    match payload {
        ObservationPayload::Wifi(w) => match_wifi(condition, w),
        ObservationPayload::Ble(b) => match condition {
            MatchCondition::MacExact { mac, .. } => {
                (b.address? == *mac).then(|| format!("BLE address is {mac}"))
            }
            MatchCondition::MacPrefix {
                prefix,
                allow_locally_administered,
                ..
            } => {
                let addr = b.address?;
                // A resolvable private address is regenerated periodically, so
                // matching a vendor prefix against one is not evidence about
                // hardware. The advertisement's own address-type flag is
                // authoritative here and is not overridable.
                if b.address_is_random {
                    return None;
                }
                if addr.is_multicast()
                    || (addr.is_locally_administered() && !allow_locally_administered)
                {
                    return None;
                }
                prefix
                    .matches(&addr)
                    .then(|| format!("BLE address {addr} matches prefix {prefix}"))
            }
            MatchCondition::BleCompanyId { id } => {
                (b.company_id? == *id).then(|| format!("BLE company ID 0x{id:04x}"))
            }
            MatchCondition::BleServiceUuid { uuid } => b
                .service_uuids
                .iter()
                .any(|u| u.eq_ignore_ascii_case(uuid))
                .then(|| format!("BLE service UUID {uuid}")),
            MatchCondition::BleNameContains { substring } => {
                let name = b.local_name.as_ref()?;
                name.contains(substring)
                    .then(|| format!("BLE name contains {substring:?}"))
            }
            _ => None,
        },
        ObservationPayload::RemoteId(_) => None,
    }
}

fn match_wifi(condition: &MatchCondition, w: &WifiObservation) -> Option<String> {
    match condition {
        MatchCondition::MacExact { field, mac } => {
            let candidates = addresses_for(*field, w);
            candidates
                .iter()
                .any(|a| a == mac)
                .then(|| format!("{} is {mac}", field_name(*field)))
        }
        MatchCondition::MacPrefix {
            field,
            prefix,
            allow_locally_administered,
        } => {
            let candidates = addresses_for(*field, w);
            let hit = candidates.iter().find(|a| {
                // A group address is never a device identity. A randomised one
                // carries no vendor information either, unless this signature
                // deliberately targets a known fixed locally-administered
                // prefix.
                !a.is_multicast()
                    && (*allow_locally_administered || !a.is_locally_administered())
                    && prefix.matches(a)
            })?;
            Some(format!(
                "{} {hit} matches prefix {prefix}",
                field_name(*field)
            ))
        }
        MatchCondition::SsidExact { ssid } => {
            let observed = w.ssid.as_ref()?;
            (observed.display() == *ssid).then(|| format!("SSID is {ssid:?}"))
        }
        MatchCondition::SsidContains { substring } => {
            let observed = w.ssid.as_ref()?.display();
            observed
                .contains(substring)
                .then(|| format!("SSID contains {substring:?}"))
        }
        MatchCondition::SsidGlob { pattern } => {
            let observed = w.ssid.as_ref()?.display();
            crate::glob::glob_match(pattern, &observed)
                .then(|| format!("SSID matches pattern {pattern:?}"))
        }
        MatchCondition::SsidWildcard => w
            .ssid
            .as_ref()?
            .is_wildcard()
            .then(|| "wildcard (zero-length) SSID observed".to_string()),
        MatchCondition::FrameType { frame } => {
            (w.frame? == *frame).then(|| format!("frame type is {frame:?}"))
        }
        MatchCondition::InformationElement { id, prefix } => {
            let element = w.information_elements.iter().find(|ie| ie.id == *id)?;
            match prefix {
                None => Some(format!("information element {id} present")),
                Some(bytes) => element
                    .data
                    .starts_with(bytes)
                    .then(|| format!("information element {id} starts with {bytes:02x?}")),
            }
        }
        MatchCondition::VendorOui { oui } => {
            let hit = w.information_elements.iter().find_map(|ie| {
                let vendor = ie.vendor_oui()?;
                let as_mac = MacAddr::new([vendor[0], vendor[1], vendor[2], 0, 0, 0]);
                oui.matches(&as_mac).then_some(vendor)
            })?;
            Some(format!(
                "vendor-specific element with OUI {:02x}:{:02x}:{:02x}",
                hit[0], hit[1], hit[2]
            ))
        }
        MatchCondition::IeIdSequence { ids } => {
            let observed: Vec<u8> = w.information_elements.iter().map(|ie| ie.id).collect();
            (observed == *ids).then(|| format!("information-element fingerprint {ids:?} matched"))
        }
        MatchCondition::BleCompanyId { .. }
        | MatchCondition::BleServiceUuid { .. }
        | MatchCondition::BleNameContains { .. } => None,
    }
}

fn addresses_for(field: MacField, w: &WifiObservation) -> Vec<MacAddr> {
    match field {
        MacField::Any => [w.transmitter, w.bssid, w.addr3, w.receiver]
            .into_iter()
            .flatten()
            .collect(),
        MacField::Bssid => w.bssid.into_iter().collect(),
        MacField::Transmitter => w.transmitter.into_iter().collect(),
        MacField::Receiver => w.receiver.into_iter().collect(),
        MacField::Addr3 => w.addr3.into_iter().collect(),
    }
}

const fn field_name(field: MacField) -> &'static str {
    match field {
        MacField::Any => "address",
        MacField::Bssid => "BSSID",
        MacField::Transmitter => "transmitter",
        MacField::Receiver => "receiver",
        MacField::Addr3 => "addr3",
    }
}

/// Deduplicates detections per device while allowing confidence to be promoted.
///
/// The failure this exists to prevent: a device is seen once by its vendor OUI
/// alone, a cooldown suppresses further alerts for it, and the later frame that
/// would have confirmed it with a full fingerprint is silently dropped. Here a
/// repeat at the same or lower confidence is suppressed, but a strictly higher
/// confidence always reports.
#[derive(Debug, Default)]
pub struct DetectionLog {
    entries: Vec<LoggedDetection>,
}

#[derive(Debug, Clone)]
struct LoggedDetection {
    key: String,
    signature_id: String,
    confidence: Confidence,
}

/// What the log decided to do with a detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogOutcome {
    /// Not seen before: report it.
    New,
    /// Seen before at a lower confidence: report it again as an escalation.
    Escalated { from: Confidence },
    /// Already reported at this confidence or higher: stay quiet.
    Suppressed,
}

impl DetectionLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a detection for `device_key` and decide whether to surface it.
    pub fn observe(&mut self, device_key: &str, detection: &Detection) -> LogOutcome {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| e.key == device_key && e.signature_id == detection.signature_id)
        {
            if detection.confidence > existing.confidence {
                let from = existing.confidence;
                existing.confidence = detection.confidence;
                return LogOutcome::Escalated { from };
            }
            return LogOutcome::Suppressed;
        }
        self.entries.push(LoggedDetection {
            key: device_key.to_string(),
            signature_id: detection.signature_id.clone(),
            confidence: detection.confidence,
        });
        LogOutcome::New
    }

    /// Highest confidence recorded for a device across all signatures.
    pub fn best_confidence(&self, device_key: &str) -> Option<Confidence> {
        self.entries
            .iter()
            .filter(|e| e.key == device_key)
            .map(|e| e.confidence)
            .max()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("signature pack uses schema version {found}, this build understands {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("signature '{id}' has no match conditions, which would match every device")]
    NoConditions { id: String },
    #[error("duplicate signature id '{id}'")]
    DuplicateId { id: String },
    #[error("a signature has an empty id")]
    EmptyId,
    #[error("could not parse signature pack: {0}")]
    Parse(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::{InformationElement, Ssid};

    fn mac(s: &str) -> MacAddr {
        MacAddr::parse(s).unwrap()
    }

    fn prefix(s: &str) -> MacPrefix {
        MacPrefix::parse(s).unwrap()
    }

    fn wifi(obs: WifiObservation) -> ObservationPayload {
        ObservationPayload::Wifi(obs)
    }

    fn oui_signature() -> Signature {
        Signature {
            id: "test.oui".into(),
            vendor: "TestVendor".into(),
            product: None,
            technology: RadioTech::Wifi,
            conditions: vec![MatchCondition::MacPrefix {
                field: MacField::Any,
                prefix: prefix("70:b3:d5:7c:b"),
                allow_locally_administered: false,
            }],
            confidence: Confidence::Info,
            severity: Severity::Informational,
            description: "vendor prefix only".into(),
            evidence: vec![],
            last_verified: None,
            notes: None,
            enabled: true,
        }
    }

    #[test]
    fn oui_only_match_is_reported_as_weak_evidence() {
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![oui_signature()],
        };
        let mut obs = WifiObservation::empty();
        obs.bssid = Some(mac("70:b3:d5:7c:b4:01"));

        let hits = db.match_observation(&wifi(obs));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].confidence, Confidence::Info);
        assert_eq!(hits[0].severity, Severity::Informational);
        assert_eq!(hits[0].matched_fields.len(), 1);
        assert!(hits[0].matched_fields[0].contains("70:b3:d5:7c:b"));
    }

    #[test]
    fn prefix_rules_do_not_fire_on_randomised_addresses() {
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![oui_signature()],
        };
        // Locally-administered bit set: this address was synthesised, so its
        // leading bytes say nothing about the hardware.
        let mut obs = WifiObservation::empty();
        obs.bssid = Some(mac("72:b3:d5:7c:b4:01"));
        assert!(obs.bssid.unwrap().is_locally_administered());
        assert!(db.match_observation(&wifi(obs)).is_empty());
    }

    /// flock-you documents `82:6b:f2` as a Flock camera prefix and warns that
    /// a blanket "skip locally-administered" rule silently drops it. The guard
    /// stays on by default but a signature can opt out for a known fixed
    /// prefix like this one.
    #[test]
    fn signature_can_opt_in_to_a_fixed_locally_administered_prefix() {
        let observed = mac("82:6b:f2:11:22:33");
        assert!(observed.is_locally_administered());

        let guarded = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![Signature {
                id: "flock.826bf2.guarded".into(),
                conditions: vec![MatchCondition::MacPrefix {
                    field: MacField::Any,
                    prefix: prefix("82:6b:f2"),
                    allow_locally_administered: false,
                }],
                ..oui_signature()
            }],
        };
        let mut obs = WifiObservation::empty();
        obs.transmitter = Some(observed);
        assert!(guarded.match_observation(&wifi(obs.clone())).is_empty());

        let opted_in = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![Signature {
                id: "flock.826bf2".into(),
                conditions: vec![MatchCondition::MacPrefix {
                    field: MacField::Any,
                    prefix: prefix("82:6b:f2"),
                    allow_locally_administered: true,
                }],
                ..oui_signature()
            }],
        };
        assert_eq!(opted_in.match_observation(&wifi(obs)).len(), 1);
    }

    #[test]
    fn opting_in_still_never_matches_a_group_address() {
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![Signature {
                conditions: vec![MatchCondition::MacPrefix {
                    field: MacField::Any,
                    prefix: prefix("ff:ff:ff"),
                    allow_locally_administered: true,
                }],
                ..oui_signature()
            }],
        };
        let mut obs = WifiObservation::empty();
        obs.receiver = Some(mac("ff:ff:ff:ff:ff:ff"));
        assert!(db.match_observation(&wifi(obs)).is_empty());
    }

    #[test]
    fn allow_locally_administered_defaults_to_off_in_json() {
        let json = r#"{
            "schema_version": 1,
            "signatures": [{
                "id": "s", "vendor": "V", "technology": "wifi",
                "conditions": [{"type": "mac_prefix", "field": "any", "prefix": "82:6b:f2"}],
                "confidence": "info", "severity": "informational", "description": "d"
            }]
        }"#;
        let db = SignatureDb::from_json(json).unwrap();
        let mut obs = WifiObservation::empty();
        obs.transmitter = Some(mac("82:6b:f2:11:22:33"));
        assert!(db.match_observation(&wifi(obs)).is_empty());
    }

    #[test]
    fn composite_signature_requires_every_condition() {
        let sig = Signature {
            id: "test.composite".into(),
            conditions: vec![
                MatchCondition::MacPrefix {
                    field: MacField::Transmitter,
                    prefix: prefix("a8:bb:cc"),
                    allow_locally_administered: false,
                },
                MatchCondition::SsidWildcard,
                MatchCondition::FrameType {
                    frame: FrameKind::ProbeRequest,
                },
            ],
            confidence: Confidence::High,
            ..oui_signature()
        };
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![sig],
        };

        // All three conditions hold.
        let mut full = WifiObservation::empty();
        full.transmitter = Some(mac("a8:bb:cc:11:22:33"));
        full.ssid = Some(Ssid::Wildcard);
        full.frame = Some(FrameKind::ProbeRequest);
        let hits = db.match_observation(&wifi(full.clone()));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].confidence, Confidence::High);
        assert_eq!(hits[0].matched_fields.len(), 3);

        // Drop the wildcard SSID: the composite must not fire.
        let mut partial = full.clone();
        partial.ssid = Some(Ssid::from_bytes(b"CorpNet"));
        assert!(db.match_observation(&wifi(partial)).is_empty());

        // Right vendor, wrong frame type.
        let mut wrong_frame = full;
        wrong_frame.frame = Some(FrameKind::Beacon);
        assert!(db.match_observation(&wifi(wrong_frame)).is_empty());
    }

    #[test]
    fn transmitter_rule_does_not_match_a_responding_access_point() {
        let sig = Signature {
            id: "test.addr2".into(),
            conditions: vec![MatchCondition::MacPrefix {
                field: MacField::Transmitter,
                prefix: prefix("a8:bb:cc"),
                allow_locally_administered: false,
            }],
            ..oui_signature()
        };
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![sig],
        };
        // The target address appears only as the receiver: this is an AP
        // replying to the device, not the device transmitting.
        let mut obs = WifiObservation::empty();
        obs.receiver = Some(mac("a8:bb:cc:11:22:33"));
        obs.transmitter = Some(mac("11:22:33:44:55:66"));
        assert!(db.match_observation(&wifi(obs)).is_empty());
    }

    #[test]
    fn disabled_signatures_never_fire() {
        let sig = Signature {
            enabled: false,
            ..oui_signature()
        };
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![sig],
        };
        let mut obs = WifiObservation::empty();
        obs.bssid = Some(mac("70:b3:d5:7c:b4:01"));
        assert!(db.match_observation(&wifi(obs)).is_empty());
    }

    #[test]
    fn ie_fingerprint_matches_exact_element_order() {
        let sig = Signature {
            id: "test.ie".into(),
            conditions: vec![MatchCondition::IeIdSequence {
                ids: vec![0, 1, 50, 45],
            }],
            confidence: Confidence::Medium,
            ..oui_signature()
        };
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![sig],
        };

        let mut obs = WifiObservation::empty();
        for id in [0u8, 1, 50, 45] {
            obs.information_elements
                .push(InformationElement::new(id, &[]));
        }
        assert_eq!(db.match_observation(&wifi(obs.clone())).len(), 1);

        // Same elements, different order: a different firmware fingerprint.
        let mut reordered = WifiObservation::empty();
        for id in [0u8, 50, 1, 45] {
            reordered
                .information_elements
                .push(InformationElement::new(id, &[]));
        }
        assert!(db.match_observation(&wifi(reordered)).is_empty());
    }

    #[test]
    fn vendor_oui_condition_reads_the_element_payload() {
        let sig = Signature {
            id: "test.vendorie".into(),
            conditions: vec![MatchCondition::VendorOui {
                oui: prefix("00:50:f2"),
            }],
            ..oui_signature()
        };
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![sig],
        };
        let mut obs = WifiObservation::empty();
        obs.information_elements
            .push(InformationElement::new(221, &[0x00, 0x50, 0xf2, 0x04]));
        assert_eq!(db.match_observation(&wifi(obs)).len(), 1);
    }

    #[test]
    fn ble_prefix_rules_are_skipped_for_private_addresses() {
        use crate::observation::BleObservation;
        let sig = Signature {
            id: "test.ble".into(),
            technology: RadioTech::Ble,
            conditions: vec![MatchCondition::MacPrefix {
                field: MacField::Any,
                prefix: prefix("a8:bb:cc"),
                allow_locally_administered: false,
            }],
            ..oui_signature()
        };
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![sig],
        };
        let base = BleObservation {
            address: Some(mac("a8:bb:cc:11:22:33")),
            address_is_random: true,
            local_name: None,
            company_id: None,
            service_uuids: vec![],
            manufacturer_data: vec![],
            rssi_dbm: None,
        };
        assert!(
            db.match_observation(&ObservationPayload::Ble(base.clone()))
                .is_empty()
        );

        let public = BleObservation {
            address_is_random: false,
            ..base
        };
        assert_eq!(
            db.match_observation(&ObservationPayload::Ble(public)).len(),
            1
        );
    }

    #[test]
    fn signatures_only_match_their_own_technology() {
        use crate::observation::BleObservation;
        let db = SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: vec![oui_signature()], // Wi-Fi
        };
        let ble = BleObservation {
            address: Some(mac("70:b3:d5:7c:b4:01")),
            address_is_random: false,
            local_name: None,
            company_id: None,
            service_uuids: vec![],
            manufacturer_data: vec![],
            rssi_dbm: None,
        };
        assert!(
            db.match_observation(&ObservationPayload::Ble(ble))
                .is_empty()
        );
    }

    #[test]
    fn pack_with_unknown_schema_is_rejected() {
        let json = r#"{"schema_version": 999, "signatures": []}"#;
        assert!(matches!(
            SignatureDb::from_json(json),
            Err(SignatureError::UnsupportedSchema { found: 999, .. })
        ));
    }

    #[test]
    fn signature_without_conditions_is_rejected() {
        let json = r#"{
            "schema_version": 1,
            "signatures": [{
                "id": "bad", "vendor": "V", "technology": "wifi",
                "conditions": [], "confidence": "info", "severity": "informational",
                "description": "matches everything"
            }]
        }"#;
        assert!(matches!(
            SignatureDb::from_json(json),
            Err(SignatureError::NoConditions { .. })
        ));
    }

    #[test]
    fn duplicate_signature_ids_are_rejected() {
        let json = r#"{
            "schema_version": 1,
            "signatures": [
                {"id": "dup", "vendor": "V", "technology": "wifi",
                 "conditions": [{"type": "ssid_wildcard"}],
                 "confidence": "info", "severity": "informational", "description": "a"},
                {"id": "dup", "vendor": "V", "technology": "wifi",
                 "conditions": [{"type": "ssid_wildcard"}],
                 "confidence": "info", "severity": "informational", "description": "b"}
            ]
        }"#;
        assert!(matches!(
            SignatureDb::from_json(json),
            Err(SignatureError::DuplicateId { .. })
        ));
    }

    #[test]
    fn nibble_prefix_survives_a_json_round_trip() {
        let json = r#"{
            "schema_version": 1,
            "signatures": [{
                "id": "keyw", "vendor": "KeyW", "technology": "wifi",
                "conditions": [{"type": "mac_prefix", "field": "any", "prefix": "70:b3:d5:7c:b"}],
                "confidence": "info", "severity": "informational",
                "description": "KeyW Corporation MA-S allocation"
            }]
        }"#;
        let db = SignatureDb::from_json(json).unwrap();
        let mut inside = WifiObservation::empty();
        inside.bssid = Some(mac("70:b3:d5:7c:be:ef"));
        assert_eq!(db.match_observation(&wifi(inside)).len(), 1);

        let mut outside = WifiObservation::empty();
        outside.bssid = Some(mac("70:b3:d5:7c:ae:ef"));
        assert!(db.match_observation(&wifi(outside)).is_empty());
    }

    // The behaviour flock-you preserves deliberately: an early weak hit must
    // not stop a later, better-evidenced one from being reported.
    #[test]
    fn weak_detection_does_not_suppress_a_later_strong_one() {
        let mut log = DetectionLog::new();
        let weak = Detection {
            signature_id: "flock.oui".into(),
            vendor: "Flock".into(),
            product: None,
            technology: RadioTech::Wifi,
            confidence: Confidence::Info,
            severity: Severity::Informational,
            matched_fields: vec!["prefix".into()],
        };
        let strong = Detection {
            confidence: Confidence::High,
            ..weak.clone()
        };

        assert_eq!(log.observe("dev-1", &weak), LogOutcome::New);
        assert_eq!(log.observe("dev-1", &weak), LogOutcome::Suppressed);
        assert_eq!(
            log.observe("dev-1", &strong),
            LogOutcome::Escalated {
                from: Confidence::Info
            }
        );
        // Having escalated, a weak repeat stays quiet.
        assert_eq!(log.observe("dev-1", &weak), LogOutcome::Suppressed);
        assert_eq!(log.best_confidence("dev-1"), Some(Confidence::High));
    }

    #[test]
    fn detection_log_keys_devices_independently() {
        let mut log = DetectionLog::new();
        let d = Detection {
            signature_id: "s".into(),
            vendor: "V".into(),
            product: None,
            technology: RadioTech::Wifi,
            confidence: Confidence::Low,
            severity: Severity::Low,
            matched_fields: vec![],
        };
        assert_eq!(log.observe("dev-1", &d), LogOutcome::New);
        assert_eq!(log.observe("dev-2", &d), LogOutcome::New);
        assert_eq!(log.observe("dev-1", &d), LogOutcome::Suppressed);
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn confidence_orders_from_info_to_high() {
        assert!(Confidence::Info < Confidence::Low);
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
    }
}
