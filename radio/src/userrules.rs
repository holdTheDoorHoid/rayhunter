//! Rules a user writes themselves, from the web interface.
//!
//! Kept in a separate file and a separate type from the curated pack, which is
//! the direction the maintainers proposed on EFForg/rayhunter#1042. A user
//! editing their own rules can never corrupt the shipped signatures, and a
//! signature-pack update can never silently drop a rule someone added.
//!
//! Everything here treats its input as hostile. A rule arrives over HTTP from
//! a browser, so it is validated against hard bounds before it is stored, and
//! it can express only matching — there is no expression language, no regular
//! expressions and nothing that can execute. The most powerful construct is a
//! bounded glob, implemented with a two-pointer scan that has no backtracking
//! blow-up, so there is no ReDoS surface to reason about.

use crate::mac::{MacAddr, MacPrefix};
use crate::observation::RadioTech;
use crate::signature::{Confidence, MacField, MatchCondition, Severity, Signature};
use serde::{Deserialize, Serialize};

/// Hard limits. These are not style preferences: they are what stops a
/// crafted rule set from exhausting a 160 MB device.
pub mod limits {
    pub const MAX_RULES: usize = 256;
    pub const MAX_NAME: usize = 64;
    pub const MAX_DESCRIPTION: usize = 512;
    pub const MAX_NOTES: usize = 512;
    pub const MAX_CRITERIA_PER_RULE: usize = 8;
    pub const MAX_PATTERN: usize = 64;
    pub const MAX_WILDCARDS: usize = 4;
    pub const MAX_ALLOWLIST: usize = 256;
    pub const MAX_COOLDOWN_SECS: u32 = 86_400;
}

/// What a user rule can match on. A deliberately smaller vocabulary than the
/// builtin [`MatchCondition`]: enough for the cases the requirements name,
/// without exposing frame internals that are easy to get subtly wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserCriterion {
    /// One exact address.
    Mac { mac: String },
    /// An address prefix of any nibble length.
    MacPrefix { prefix: String },
    /// The network name, exactly.
    SsidExact { ssid: String },
    /// The network name contains this text.
    SsidContains { substring: String },
    /// The network name matches a glob: `*` for any run, `?` for one
    /// character. Bounded in both length and wildcard count.
    SsidGlob { pattern: String },
    /// A Bluetooth company identifier.
    BleCompanyId { id: u16 },
    /// A Bluetooth service UUID.
    BleServiceUuid { uuid: String },
    /// The Bluetooth device name contains this text.
    BleNameContains { substring: String },
}

/// A rule as the user wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserRule {
    /// Stable identifier, assigned by the daemon rather than the browser.
    pub id: String,
    /// What the user calls this rule.
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub technology: RadioTech,
    pub severity: Severity,
    /// All criteria must match, so a user can build a composite rule.
    pub criteria: Vec<UserCriterion>,
    /// Seconds to stay quiet after alerting on the same device.
    #[serde(default)]
    pub cooldown_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

const fn default_true() -> bool {
    true
}

/// A user's rules plus the devices they never want to hear about.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserRuleSet {
    #[serde(default)]
    pub rules: Vec<UserRule>,
    /// Devices to ignore entirely: the user's own equipment, a housemate's
    /// camera, anything already explained.
    #[serde(default)]
    pub allowlist: Vec<AllowlistEntry>,
}

/// One entry on the ignore list, with the user's own note about why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllowlistEntry {
    pub prefix: String,
    #[serde(default)]
    pub label: String,
}

impl UserRuleSet {
    /// Parse and fully validate a rule set. Returns the first problem found,
    /// phrased for display next to the field the user was editing.
    pub fn from_json(input: &str) -> Result<Self, RuleError> {
        let set: UserRuleSet = serde_json::from_str(input)?;
        set.validate()?;
        Ok(set)
    }

    pub fn validate(&self) -> Result<(), RuleError> {
        if self.rules.len() > limits::MAX_RULES {
            return Err(RuleError::TooManyRules {
                count: self.rules.len(),
                max: limits::MAX_RULES,
            });
        }
        if self.allowlist.len() > limits::MAX_ALLOWLIST {
            return Err(RuleError::TooManyAllowlistEntries {
                count: self.allowlist.len(),
                max: limits::MAX_ALLOWLIST,
            });
        }
        let mut seen: Vec<&str> = Vec::with_capacity(self.rules.len());
        for rule in &self.rules {
            rule.validate()?;
            if seen.contains(&rule.id.as_str()) {
                return Err(RuleError::DuplicateId {
                    id: rule.id.clone(),
                });
            }
            seen.push(&rule.id);
        }
        for entry in &self.allowlist {
            MacPrefix::parse(&entry.prefix).map_err(|e| RuleError::BadPrefix {
                value: entry.prefix.clone(),
                reason: e.to_string(),
            })?;
            check_len("allowlist label", &entry.label, limits::MAX_NAME)?;
        }
        Ok(())
    }

    /// True when this address is on the ignore list.
    pub fn is_allowlisted(&self, addr: &MacAddr) -> bool {
        self.allowlist.iter().any(|entry| {
            MacPrefix::parse(&entry.prefix)
                .map(|p| p.matches(addr))
                .unwrap_or(false)
        })
    }

    /// Convert the enabled rules into signatures the matcher understands.
    ///
    /// User rule IDs are namespaced under `user.` so a detection's origin is
    /// unambiguous in evidence and in the UI: nobody should have to wonder
    /// whether an alert came from the curated pack or from something they
    /// typed in themselves.
    pub fn to_signatures(&self) -> Result<Vec<Signature>, RuleError> {
        self.rules
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.to_signature())
            .collect()
    }
}

impl UserRule {
    pub fn validate(&self) -> Result<(), RuleError> {
        if self.id.trim().is_empty() {
            return Err(RuleError::EmptyField { field: "id" });
        }
        if self.name.trim().is_empty() {
            return Err(RuleError::EmptyField { field: "name" });
        }
        check_len("name", &self.name, limits::MAX_NAME)?;
        check_len("description", &self.description, limits::MAX_DESCRIPTION)?;
        if let Some(notes) = &self.notes {
            check_len("notes", notes, limits::MAX_NOTES)?;
        }
        if self.criteria.is_empty() {
            return Err(RuleError::NoCriteria {
                name: self.name.clone(),
            });
        }
        if self.criteria.len() > limits::MAX_CRITERIA_PER_RULE {
            return Err(RuleError::TooManyCriteria {
                name: self.name.clone(),
                max: limits::MAX_CRITERIA_PER_RULE,
            });
        }
        if self.cooldown_secs > limits::MAX_COOLDOWN_SECS {
            return Err(RuleError::CooldownTooLong {
                secs: self.cooldown_secs,
                max: limits::MAX_COOLDOWN_SECS,
            });
        }
        for criterion in &self.criteria {
            validate_criterion(criterion)?;
        }
        Ok(())
    }

    fn to_signature(&self) -> Result<Signature, RuleError> {
        let conditions = self
            .criteria
            .iter()
            .map(convert_criterion)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Signature {
            id: format!("user.{}", self.id),
            vendor: "user-defined".to_string(),
            product: Some(self.name.clone()),
            technology: self.technology,
            conditions,
            // A user asserting a device is interesting is a stronger statement
            // than a vendor-prefix guess, but it is still one signal. The
            // severity the user chose controls how loudly it is surfaced.
            confidence: Confidence::Low,
            severity: self.severity,
            description: if self.description.trim().is_empty() {
                format!("user rule: {}", self.name)
            } else {
                self.description.clone()
            },
            evidence: vec!["added by the device owner".to_string()],
            last_verified: None,
            notes: self.notes.clone(),
            enabled: true,
        })
    }
}

fn validate_criterion(criterion: &UserCriterion) -> Result<(), RuleError> {
    match criterion {
        UserCriterion::Mac { mac } => {
            MacAddr::parse(mac).map_err(|e| RuleError::BadMac {
                value: mac.clone(),
                reason: e.to_string(),
            })?;
        }
        UserCriterion::MacPrefix { prefix } => {
            MacPrefix::parse(prefix).map_err(|e| RuleError::BadPrefix {
                value: prefix.clone(),
                reason: e.to_string(),
            })?;
        }
        UserCriterion::SsidExact { ssid } => check_len("SSID", ssid, limits::MAX_PATTERN)?,
        UserCriterion::SsidContains { substring } => {
            if substring.is_empty() {
                return Err(RuleError::EmptyField { field: "substring" });
            }
            check_len("SSID text", substring, limits::MAX_PATTERN)?;
        }
        UserCriterion::SsidGlob { pattern } => {
            if pattern.is_empty() {
                return Err(RuleError::EmptyField { field: "pattern" });
            }
            check_len("pattern", pattern, limits::MAX_PATTERN)?;
            let wildcards = pattern.chars().filter(|c| *c == '*' || *c == '?').count();
            if wildcards > limits::MAX_WILDCARDS {
                return Err(RuleError::TooManyWildcards {
                    count: wildcards,
                    max: limits::MAX_WILDCARDS,
                });
            }
            if pattern.chars().all(|c| c == '*') {
                return Err(RuleError::MatchesEverything);
            }
        }
        UserCriterion::BleCompanyId { .. } => {}
        UserCriterion::BleServiceUuid { uuid } => {
            check_len("UUID", uuid, limits::MAX_PATTERN)?;
            if uuid.trim().is_empty() {
                return Err(RuleError::EmptyField { field: "uuid" });
            }
        }
        UserCriterion::BleNameContains { substring } => {
            if substring.is_empty() {
                return Err(RuleError::EmptyField { field: "substring" });
            }
            check_len("name text", substring, limits::MAX_PATTERN)?;
        }
    }
    Ok(())
}

fn convert_criterion(criterion: &UserCriterion) -> Result<MatchCondition, RuleError> {
    Ok(match criterion {
        UserCriterion::Mac { mac } => MatchCondition::MacExact {
            field: MacField::Any,
            mac: MacAddr::parse(mac).map_err(|e| RuleError::BadMac {
                value: mac.clone(),
                reason: e.to_string(),
            })?,
        },
        UserCriterion::MacPrefix { prefix } => MatchCondition::MacPrefix {
            field: MacField::Any,
            prefix: MacPrefix::parse(prefix).map_err(|e| RuleError::BadPrefix {
                value: prefix.clone(),
                reason: e.to_string(),
            })?,
            // A user targeting a specific device may well be targeting one
            // that randomises, and they are asserting the address themselves
            // rather than inferring a vendor from it.
            allow_locally_administered: true,
        },
        UserCriterion::SsidExact { ssid } => MatchCondition::SsidExact { ssid: ssid.clone() },
        UserCriterion::SsidContains { substring } => MatchCondition::SsidContains {
            substring: substring.clone(),
        },
        UserCriterion::SsidGlob { pattern } => MatchCondition::SsidGlob {
            pattern: pattern.clone(),
        },
        UserCriterion::BleCompanyId { id } => MatchCondition::BleCompanyId { id: *id },
        UserCriterion::BleServiceUuid { uuid } => {
            MatchCondition::BleServiceUuid { uuid: uuid.clone() }
        }
        UserCriterion::BleNameContains { substring } => MatchCondition::BleNameContains {
            substring: substring.clone(),
        },
    })
}

fn check_len(field: &'static str, value: &str, max: usize) -> Result<(), RuleError> {
    if value.chars().count() > max {
        return Err(RuleError::TooLong {
            field,
            len: value.chars().count(),
            max,
        });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RuleError {
    #[error("{count} rules is more than the {max} allowed")]
    TooManyRules { count: usize, max: usize },
    #[error("{count} ignore-list entries is more than the {max} allowed")]
    TooManyAllowlistEntries { count: usize, max: usize },
    #[error("two rules share the id '{id}'")]
    DuplicateId { id: String },
    #[error("{field} cannot be empty")]
    EmptyField { field: &'static str },
    #[error("{field} is {len} characters, longer than the {max} allowed")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("rule '{name}' has no criteria, so it would match every device")]
    NoCriteria { name: String },
    #[error("rule '{name}' has more than the {max} criteria allowed")]
    TooManyCriteria { name: String, max: usize },
    #[error("'{value}' is not a valid MAC address: {reason}")]
    BadMac { value: String, reason: String },
    #[error("'{value}' is not a valid address prefix: {reason}")]
    BadPrefix { value: String, reason: String },
    #[error("{count} wildcards is more than the {max} allowed")]
    TooManyWildcards { count: usize, max: usize },
    #[error("that pattern would match every network")]
    MatchesEverything,
    #[error("cooldown of {secs}s is longer than the {max}s allowed")]
    CooldownTooLong { secs: u32, max: u32 },
    #[error("could not read the rules: {0}")]
    Parse(String),
}

impl From<serde_json::Error> for RuleError {
    fn from(e: serde_json::Error) -> Self {
        RuleError::Parse(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::{ObservationPayload, WifiObservation};
    use crate::signature::{SCHEMA_VERSION, SignatureDb};

    fn rule(criteria: Vec<UserCriterion>) -> UserRule {
        UserRule {
            id: "r1".into(),
            name: "The white van".into(),
            description: "Seen outside since Tuesday".into(),
            enabled: true,
            technology: RadioTech::Wifi,
            severity: Severity::Medium,
            criteria,
            cooldown_secs: 300,
            notes: None,
        }
    }

    fn db_from(set: &UserRuleSet) -> SignatureDb {
        SignatureDb {
            schema_version: SCHEMA_VERSION,
            pack_version: None,
            signatures: set.to_signatures().unwrap(),
        }
    }

    fn seen(addr: &str, ssid: Option<&str>) -> ObservationPayload {
        let mut obs = WifiObservation::empty();
        obs.bssid = Some(MacAddr::parse(addr).unwrap());
        obs.transmitter = Some(MacAddr::parse(addr).unwrap());
        obs.ssid = ssid.map(|s| crate::observation::Ssid::from_bytes(s.as_bytes()));
        ObservationPayload::Wifi(obs)
    }

    #[test]
    fn a_user_mac_rule_matches_that_device() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::Mac {
                mac: "aa:bb:cc:dd:ee:ff".into(),
            }])],
            allowlist: vec![],
        };
        let db = db_from(&set);
        let hits = db.match_observation(&seen("aa:bb:cc:dd:ee:ff", None));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].severity, Severity::Medium);
    }

    #[test]
    fn user_rules_are_namespaced_so_their_origin_is_obvious() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::MacPrefix {
                prefix: "aa:bb:cc".into(),
            }])],
            allowlist: vec![],
        };
        let sigs = set.to_signatures().unwrap();
        assert_eq!(sigs[0].id, "user.r1");
        assert_eq!(sigs[0].vendor, "user-defined");
    }

    /// A user naming a specific device may well be naming one that
    /// randomises, so their rule is not subject to the vendor-prefix guard.
    #[test]
    fn user_prefix_rules_still_match_randomised_addresses() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::MacPrefix {
                prefix: "aa:bb:cc".into(),
            }])],
            allowlist: vec![],
        };
        let db = db_from(&set);
        assert!(
            MacAddr::parse("aa:bb:cc:dd:ee:ff")
                .unwrap()
                .is_locally_administered()
        );
        assert_eq!(
            db.match_observation(&seen("aa:bb:cc:dd:ee:ff", None)).len(),
            1
        );
    }

    #[test]
    fn ssid_substring_rule_matches() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::SsidContains {
                substring: "FlockSafety".into(),
            }])],
            allowlist: vec![],
        };
        let db = db_from(&set);
        assert_eq!(
            db.match_observation(&seen("11:22:33:44:55:66", Some("FlockSafety-1234")))
                .len(),
            1
        );
        assert!(
            db.match_observation(&seen("11:22:33:44:55:66", Some("HomeWiFi")))
                .is_empty()
        );
    }

    /// A glob rule has to reach the matcher as a glob. An earlier version
    /// validated the pattern and then converted it to a plain substring
    /// match, so a wildcard in the middle silently never matched anything.
    #[test]
    fn a_glob_rule_matches_as_a_glob_not_as_a_substring() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::SsidGlob {
                pattern: "Flock?afety*".into(),
            }])],
            allowlist: vec![],
        };
        let db = db_from(&set);

        let hits = db.match_observation(&seen("11:22:33:44:55:66", Some("FlockSafety-1234")));
        assert_eq!(hits.len(), 1, "the wildcard pattern should have matched");
        assert!(hits[0].matched_fields[0].contains("Flock?afety*"));

        assert!(
            db.match_observation(&seen("11:22:33:44:55:66", Some("FlockXafet")))
                .is_empty()
        );
        assert!(
            db.match_observation(&seen("11:22:33:44:55:66", Some("HomeWiFi")))
                .is_empty()
        );
    }

    #[test]
    fn a_trailing_star_glob_still_anchors_at_the_start() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::SsidGlob {
                pattern: "cam-*".into(),
            }])],
            allowlist: vec![],
        };
        let db = db_from(&set);
        assert_eq!(
            db.match_observation(&seen("11:22:33:44:55:66", Some("cam-backyard")))
                .len(),
            1
        );
        // Anchored: a name merely containing "cam-" must not match.
        assert!(
            db.match_observation(&seen("11:22:33:44:55:66", Some("my-cam-backyard")))
                .is_empty()
        );
    }

    #[test]
    fn allowlist_silences_a_device() {
        let set = UserRuleSet {
            rules: vec![],
            allowlist: vec![AllowlistEntry {
                prefix: "58:32:77".into(),
                label: "my own hotspot".into(),
            }],
        };
        assert!(set.is_allowlisted(&MacAddr::parse("58:32:77:28:7b:a6").unwrap()));
        assert!(!set.is_allowlisted(&MacAddr::parse("70:b3:d5:7c:b4:01").unwrap()));
    }

    #[test]
    fn a_rule_with_no_criteria_is_rejected() {
        let set = UserRuleSet {
            rules: vec![rule(vec![])],
            allowlist: vec![],
        };
        assert!(matches!(set.validate(), Err(RuleError::NoCriteria { .. })));
    }

    #[test]
    fn an_unparseable_address_is_rejected_with_a_readable_reason() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::Mac {
                mac: "not-a-mac".into(),
            }])],
            allowlist: vec![],
        };
        let err = set.validate().unwrap_err();
        assert!(matches!(err, RuleError::BadMac { .. }));
        assert!(err.to_string().contains("not-a-mac"));
    }

    #[test]
    fn oversized_fields_are_rejected() {
        let mut r = rule(vec![UserCriterion::MacPrefix {
            prefix: "aa:bb:cc".into(),
        }]);
        r.name = "x".repeat(limits::MAX_NAME + 1);
        let set = UserRuleSet {
            rules: vec![r],
            allowlist: vec![],
        };
        assert!(matches!(set.validate(), Err(RuleError::TooLong { .. })));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let a = rule(vec![UserCriterion::MacPrefix {
            prefix: "aa:bb:cc".into(),
        }]);
        let b = a.clone();
        let set = UserRuleSet {
            rules: vec![a, b],
            allowlist: vec![],
        };
        assert!(matches!(set.validate(), Err(RuleError::DuplicateId { .. })));
    }

    #[test]
    fn too_many_rules_are_rejected() {
        let rules = (0..limits::MAX_RULES + 1)
            .map(|i| UserRule {
                id: format!("r{i}"),
                ..rule(vec![UserCriterion::MacPrefix {
                    prefix: "aa:bb:cc".into(),
                }])
            })
            .collect();
        let set = UserRuleSet {
            rules,
            allowlist: vec![],
        };
        assert!(matches!(
            set.validate(),
            Err(RuleError::TooManyRules { .. })
        ));
    }

    #[test]
    fn a_pattern_of_only_wildcards_is_rejected() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::SsidGlob {
                pattern: "***".into(),
            }])],
            allowlist: vec![],
        };
        assert!(matches!(set.validate(), Err(RuleError::MatchesEverything)));
    }

    #[test]
    fn wildcard_count_is_capped() {
        let set = UserRuleSet {
            rules: vec![rule(vec![UserCriterion::SsidGlob {
                pattern: "a*b*c*d*e*f".into(),
            }])],
            allowlist: vec![],
        };
        assert!(matches!(
            set.validate(),
            Err(RuleError::TooManyWildcards { .. })
        ));
    }

    #[test]
    fn disabled_rules_produce_no_signature() {
        let mut r = rule(vec![UserCriterion::MacPrefix {
            prefix: "aa:bb:cc".into(),
        }]);
        r.enabled = false;
        let set = UserRuleSet {
            rules: vec![r],
            allowlist: vec![],
        };
        assert!(set.to_signatures().unwrap().is_empty());
    }

    #[test]
    fn rules_round_trip_through_json() {
        let set = UserRuleSet {
            rules: vec![rule(vec![
                UserCriterion::MacPrefix {
                    prefix: "70:b3:d5:7c:b".into(),
                },
                UserCriterion::SsidContains {
                    substring: "cam".into(),
                },
            ])],
            allowlist: vec![AllowlistEntry {
                prefix: "58:32:77".into(),
                label: "mine".into(),
            }],
        };
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(UserRuleSet::from_json(&json).unwrap(), set);
    }

    #[test]
    fn a_config_file_cannot_smuggle_in_executable_content() {
        // There is no field that takes a command, a path or an expression.
        // An unknown key is simply not part of the schema and fails to parse
        // into a criterion rather than being carried along.
        let json = r#"{
            "rules": [{
                "id": "r1", "name": "n", "technology": "wifi", "severity": "low",
                "criteria": [{"type": "exec", "command": "/bin/sh"}]
            }]
        }"#;
        assert!(UserRuleSet::from_json(json).is_err());
    }

    #[test]
    fn a_hostile_rule_name_is_stored_verbatim_for_the_ui_to_escape() {
        let mut r = rule(vec![UserCriterion::MacPrefix {
            prefix: "aa:bb:cc".into(),
        }]);
        r.name = "<script>alert(1)</script>".into();
        let set = UserRuleSet {
            rules: vec![r],
            allowlist: vec![],
        };
        // Validation is about bounds, not about mangling the user's text.
        // Escaping belongs to the presentation layer, which must do it.
        assert!(set.validate().is_ok());
        let sigs = set.to_signatures().unwrap();
        assert_eq!(
            sigs[0].product.as_deref(),
            Some("<script>alert(1)</script>")
        );
    }

    #[test]
    fn cooldown_is_bounded() {
        let mut r = rule(vec![UserCriterion::MacPrefix {
            prefix: "aa:bb:cc".into(),
        }]);
        r.cooldown_secs = limits::MAX_COOLDOWN_SECS + 1;
        let set = UserRuleSet {
            rules: vec![r],
            allowlist: vec![],
        };
        assert!(matches!(
            set.validate(),
            Err(RuleError::CooldownTooLong { .. })
        ));
    }
}
