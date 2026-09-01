//! Wireless surveillance detection for Rayhunter.
//!
//! This crate is deliberately free of I/O, device access and cellular
//! concerns. It defines what a radio observation *is*, decides which
//! observations match a surveillance signature, and says how the resulting
//! evidence is written down. A companion daemon owns the radio hardware and
//! feeds this crate; the Rayhunter cellular daemon never links against the
//! hardware paths at all, so a fault in radio capture cannot take down
//! `/dev/diag` processing.
//!
//! The split follows the maintainers' guidance on EFForg/rayhunter#888, that
//! Wi-Fi handling should live outside the Rayhunter daemon rather than being
//! coupled to it.
//!
//! # Layout
//!
//! * [`mac`] — addresses and nibble-precision prefixes
//! * [`observation`] — what a radio saw, independent of which radio saw it
//! * [`signature`] — data-driven rules, confidence, and escalation
//! * [`scan`] — the BSS-scan capture backend and its limits
//! * [`persistence`] — whether a device has stayed near the user as they moved
//! * [`evidence`] — durable NDJSON records and their retention policy

pub mod evidence;
pub mod mac;
pub mod observation;
pub mod persistence;
pub mod scan;
pub mod signature;
pub mod userrules;

pub use mac::{MacAddr, MacPrefix};
pub use observation::{
    ObservationPayload, ObservationSource, RadioObservation, RadioTech, WifiObservation,
};
pub use persistence::{EnvironmentTracker, Persistence, PersistenceScore, PersistenceTracker};
pub use signature::{Confidence, Detection, DetectionLog, Severity, SignatureDb};
pub use userrules::{UserRule, UserRuleSet};
