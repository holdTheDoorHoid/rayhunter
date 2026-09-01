//! Has this device stayed near me as I moved?
//!
//! "Is it nearby" is a question a scan answers. "Has it followed me" is a
//! question about time and place, and it is the one that distinguishes a
//! neighbour's doorbell camera from something worth worrying about.
//!
//! Two design commitments shape this module.
//!
//! **Movement is inferred, not located.** Scoring needs to know that the user
//! changed environment, not where they went. [`EnvironmentTracker`] decides
//! that from how much the surrounding set of access points changed, so
//! following-detection works with location recording switched off — which is
//! the default. GNSS, if it is ever wired in, is an optional refinement and
//! never a prerequisite.
//!
//! **Persistence alone is not following.** A device seen constantly in exactly
//! one place is a fixture: a neighbour's router, a shop's access point, the
//! user's own equipment. [`PersistenceTracker`] will not classify anything
//! above [`Persistence::UnusuallyPersistent`] unless it has been seen in more
//! than one distinct environment, however dense the sightings are.
//!
//! # What this cannot do
//!
//! A device that rotates its hardware address between sightings appears as a
//! new device each time and will never accumulate a score. Modern phones do
//! this by default. Correlating rotated identities needs stable fingerprints
//! from frames this platform cannot capture, so the honest statement to a user
//! is that following-detection covers devices with stable addresses and says
//! nothing about the rest.

use crate::mac::MacAddr;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How a device's pattern of sightings should be described to the user.
///
/// Ordered from least to most concerning so a caller can threshold on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    /// Seen in passing. The overwhelming majority of devices.
    Incidental,
    /// Around for a while, but in one place. A fixture, most likely.
    Persistent,
    /// Present far more than a passing device would be, still in one place.
    UnusuallyPersistent,
    /// Seen across several distinct environments as the user moved.
    PossibleFollowing,
    /// Seen across many environments, repeatedly, over a long span.
    HighConfidenceFollowing,
}

impl Persistence {
    pub const fn label(&self) -> &'static str {
        match self {
            Persistence::Incidental => "incidental",
            Persistence::Persistent => "persistent",
            Persistence::UnusuallyPersistent => "unusually persistent",
            Persistence::PossibleFollowing => "possible following behaviour",
            Persistence::HighConfidenceFollowing => "correlated following behaviour",
        }
    }
}

/// Tunable thresholds. Defaults are deliberately conservative: this feature
/// tells someone they may be being followed, and a false positive there is
/// not a minor annoyance.
#[derive(Debug, Clone, Copy)]
pub struct PersistenceConfig {
    /// Sightings below which nothing is scored at all.
    pub min_sightings: u32,
    /// Span, in hours, at which the temporal component saturates.
    pub temporal_saturation_hours: f32,
    /// A gap longer than this counts as the device having left and returned.
    pub reappearance_gap: Duration,
    /// Distinct environments at which the movement component saturates.
    pub environment_saturation: u32,
    /// Sighting count at which the density component saturates.
    pub density_saturation: u32,
    /// Most devices held in memory at once.
    pub max_tracked_devices: usize,
    /// Most sightings retained per device.
    pub max_sightings_per_device: usize,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        PersistenceConfig {
            min_sightings: 3,
            temporal_saturation_hours: 4.0,
            reappearance_gap: Duration::minutes(10),
            environment_saturation: 4,
            density_saturation: 20,
            max_tracked_devices: 512,
            max_sightings_per_device: 64,
        }
    }
}

/// A scored assessment, with the reasons that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistenceScore {
    /// 0.0 to 1.0.
    pub score: f32,
    pub classification: Persistence,
    pub sightings: u32,
    pub distinct_environments: u32,
    pub reappearances: u32,
    pub span_hours: f32,
    /// Human-readable, surfaced verbatim in the UI.
    pub reasons: Vec<String>,
}

/// Decides when the user has changed surroundings, without recording where.
///
/// The current environment is the set of access points recently visible. When
/// a new scan overlaps that set too little, the user has moved somewhere else
/// and the environment counter advances. Only the counter and a rolling set of
/// BSSIDs are held, both in memory; nothing about location is written down.
#[derive(Debug)]
pub struct EnvironmentTracker {
    current: Vec<MacAddr>,
    index: u32,
    /// Overlap below which the surroundings count as different, as a fraction
    /// of the smaller set (0.0 to 1.0).
    similarity_threshold: f32,
    /// Scans with fewer access points than this are too sparse to judge, and
    /// are folded into the current environment rather than inventing a move.
    min_anchors: usize,
}

impl Default for EnvironmentTracker {
    fn default() -> Self {
        EnvironmentTracker {
            current: Vec::new(),
            index: 0,
            similarity_threshold: 0.5,
            min_anchors: 3,
        }
    }
}

impl EnvironmentTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index of the environment the user is currently in.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Feed the access points seen in one scan. Returns true if this looks
    /// like a different place from the previous scan.
    pub fn observe(&mut self, visible: &[MacAddr]) -> bool {
        if visible.len() < self.min_anchors {
            // Too little to tell. Saying "moved" here would manufacture
            // environment changes every time a scan came back thin.
            return false;
        }
        if self.current.is_empty() {
            self.current = visible.to_vec();
            return false;
        }

        let overlap = visible.iter().filter(|m| self.current.contains(m)).count();
        let smaller = self.current.len().min(visible.len()) as f32;
        let similarity = overlap as f32 / smaller;

        self.current = visible.to_vec();
        if similarity < self.similarity_threshold {
            self.index = self.index.saturating_add(1);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
struct Sighting {
    at: DateTime<Utc>,
    environment: u32,
    rssi_dbm: Option<i16>,
}

#[derive(Debug, Clone)]
struct DeviceHistory {
    sightings: Vec<Sighting>,
    last_seen: DateTime<Utc>,
}

/// Accumulates sightings per device and scores how persistently each has been
/// present. Memory is bounded on both axes: a cap on devices tracked, and a
/// cap on sightings kept per device.
#[derive(Debug)]
pub struct PersistenceTracker {
    config: PersistenceConfig,
    devices: HashMap<String, DeviceHistory>,
}

impl PersistenceTracker {
    pub fn new(config: PersistenceConfig) -> Self {
        PersistenceTracker {
            config,
            devices: HashMap::new(),
        }
    }

    pub fn tracked_devices(&self) -> usize {
        self.devices.len()
    }

    /// Record that `key` was seen. `key` is whatever identity the caller uses
    /// — a MAC for a matched device, a session pseudonym otherwise — so this
    /// works without the tracker ever handling a real address.
    pub fn record(
        &mut self,
        key: &str,
        at: DateTime<Utc>,
        environment: u32,
        rssi_dbm: Option<i16>,
    ) {
        if !self.devices.contains_key(key) && self.devices.len() >= self.config.max_tracked_devices
        {
            self.evict_oldest();
        }
        let entry = self
            .devices
            .entry(key.to_string())
            .or_insert_with(|| DeviceHistory {
                sightings: Vec::new(),
                last_seen: at,
            });
        entry.sightings.push(Sighting {
            at,
            environment,
            rssi_dbm,
        });
        if entry.sightings.len() > self.config.max_sightings_per_device {
            // Drop the oldest. The span is preserved well enough by what
            // remains, and an unbounded history on a 160 MB device is not an
            // option.
            entry.sightings.remove(0);
        }
        if at > entry.last_seen {
            entry.last_seen = at;
        }
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest) = self
            .devices
            .iter()
            .min_by_key(|(_, h)| h.last_seen)
            .map(|(k, _)| k.clone())
        {
            self.devices.remove(&oldest);
        }
    }

    /// Score one device, or `None` if it has not been seen enough to judge.
    pub fn score(&self, key: &str) -> Option<PersistenceScore> {
        let history = self.devices.get(key)?;
        let sightings = history.sightings.len() as u32;
        if sightings < self.config.min_sightings {
            return None;
        }

        let first = history.sightings.first()?.at;
        let last = history.sightings.last()?.at;
        let span_hours = (last - first).num_seconds().max(0) as f32 / 3600.0;

        let mut environments: Vec<u32> = history.sightings.iter().map(|s| s.environment).collect();
        environments.sort_unstable();
        environments.dedup();
        let distinct_environments = environments.len() as u32;

        let mut reappearances = 0u32;
        for pair in history.sightings.windows(2) {
            if pair[1].at - pair[0].at > self.config.reappearance_gap {
                reappearances += 1;
            }
        }

        let temporal = (span_hours / self.config.temporal_saturation_hours).clamp(0.0, 1.0);
        let recurrence = (reappearances as f32 / 5.0).clamp(0.0, 1.0);
        let movement = if self.config.environment_saturation > 1 {
            (distinct_environments.saturating_sub(1) as f32
                / (self.config.environment_saturation - 1) as f32)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let density = (sightings as f32 / self.config.density_saturation as f32).clamp(0.0, 1.0);

        // Movement carries the most weight: it is the component that
        // distinguishes following from merely being a fixture.
        let score = (0.40 * movement + 0.25 * temporal + 0.25 * recurrence + 0.10 * density)
            .clamp(0.0, 1.0);

        let mut reasons = Vec::new();
        reasons.push(format!("seen {sightings} times over {span_hours:.1} hours"));
        if distinct_environments > 1 {
            reasons.push(format!(
                "present in {distinct_environments} distinct surroundings as you moved"
            ));
        } else {
            reasons.push("only ever seen in one set of surroundings".to_string());
        }
        if reappearances > 0 {
            reasons.push(format!("left and returned {reappearances} times"));
        }
        if let Some(trend) = rssi_summary(&history.sightings) {
            reasons.push(trend);
        }

        let classification = classify(score, distinct_environments);
        if distinct_environments <= 1 && score >= 0.6 {
            reasons.push(
                "not treated as following: a device seen in only one place is most likely a fixture"
                    .to_string(),
            );
        }

        Some(PersistenceScore {
            score,
            classification,
            sightings,
            distinct_environments,
            reappearances,
            span_hours,
            reasons,
        })
    }

    /// Every device currently scoring at or above `floor`, strongest first.
    pub fn above(&self, floor: Persistence) -> Vec<(String, PersistenceScore)> {
        let mut out: Vec<(String, PersistenceScore)> = self
            .devices
            .keys()
            .filter_map(|k| self.score(k).map(|s| (k.clone(), s)))
            .filter(|(_, s)| s.classification >= floor)
            .collect();
        out.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Forget sightings older than `cutoff`, and devices left with none.
    pub fn prune(&mut self, cutoff: DateTime<Utc>) {
        self.devices.retain(|_, h| {
            h.sightings.retain(|s| s.at >= cutoff);
            !h.sightings.is_empty()
        });
    }
}

/// Map a score to a classification, refusing to call anything "following"
/// when it has only ever been seen in one place.
fn classify(score: f32, distinct_environments: u32) -> Persistence {
    if distinct_environments <= 1 {
        return if score >= 0.4 {
            Persistence::UnusuallyPersistent
        } else if score >= 0.2 {
            Persistence::Persistent
        } else {
            Persistence::Incidental
        };
    }
    if score >= 0.8 {
        Persistence::HighConfidenceFollowing
    } else if score >= 0.6 {
        Persistence::PossibleFollowing
    } else if score >= 0.4 {
        Persistence::UnusuallyPersistent
    } else if score >= 0.2 {
        Persistence::Persistent
    } else {
        Persistence::Incidental
    }
}

/// Describe the signal trend, when there is enough of it to mean anything.
fn rssi_summary(sightings: &[Sighting]) -> Option<String> {
    let values: Vec<i16> = sightings.iter().filter_map(|s| s.rssi_dbm).collect();
    if values.len() < 3 {
        return None;
    }
    let min = *values.iter().min()?;
    let max = *values.iter().max()?;
    let mean = values.iter().map(|v| *v as i32).sum::<i32>() / values.len() as i32;
    Some(format!(
        "signal averaged {mean} dBm, ranging {min} to {max}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(s: &str) -> MacAddr {
        MacAddr::parse(s).unwrap()
    }

    fn t(minutes: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + Duration::minutes(minutes)
    }

    fn env_set(prefix: &str, n: u8) -> Vec<MacAddr> {
        (0..n)
            .map(|i| mac(&format!("{prefix}:00:00:{i:02x}")))
            .collect()
    }

    #[test]
    fn a_stable_environment_does_not_register_a_move() {
        let mut env = EnvironmentTracker::new();
        let home = env_set("aa:aa:aa", 6);
        assert!(!env.observe(&home));
        assert!(!env.observe(&home));
        assert_eq!(env.index(), 0);
    }

    #[test]
    fn a_wholly_different_set_of_access_points_is_a_new_environment() {
        let mut env = EnvironmentTracker::new();
        env.observe(&env_set("aa:aa:aa", 6));
        assert!(env.observe(&env_set("bb:bb:bb", 6)));
        assert_eq!(env.index(), 1);
    }

    #[test]
    fn partial_overlap_is_treated_as_the_same_place() {
        let mut env = EnvironmentTracker::new();
        env.observe(&env_set("aa:aa:aa", 6));
        // Four of six still visible: the user has not gone anywhere.
        let mut drifted = env_set("aa:aa:aa", 4);
        drifted.extend(env_set("cc:cc:cc", 2));
        assert!(!env.observe(&drifted));
        assert_eq!(env.index(), 0);
    }

    #[test]
    fn a_thin_scan_does_not_invent_a_move() {
        let mut env = EnvironmentTracker::new();
        env.observe(&env_set("aa:aa:aa", 6));
        // One access point visible: not enough to conclude anything.
        assert!(!env.observe(&env_set("zz", 0)));
        assert!(!env.observe(&[mac("dd:dd:dd:00:00:01")]));
        assert_eq!(env.index(), 0);
    }

    #[test]
    fn too_few_sightings_are_not_scored() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        tracker.record("dev", t(0), 0, Some(-50));
        tracker.record("dev", t(5), 0, Some(-50));
        assert!(tracker.score("dev").is_none());
    }

    /// The neighbour's doorbell camera: always there, never anywhere else.
    #[test]
    fn a_fixture_is_never_classified_as_following() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        for i in 0..40 {
            tracker.record("fixture", t(i * 15), 0, Some(-45));
        }
        let score = tracker.score("fixture").unwrap();
        assert_eq!(score.distinct_environments, 1);
        assert!(
            score.classification <= Persistence::UnusuallyPersistent,
            "a device seen in one place must not be called following, got {:?}",
            score.classification
        );
        assert!(
            score
                .reasons
                .iter()
                .any(|r| r.contains("only ever seen in one set of surroundings"))
        );
    }

    /// The case the feature exists for: present across several environments,
    /// repeatedly, over hours.
    #[test]
    fn a_device_across_many_environments_scores_as_following() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        let mut minute = 0;
        for environment in 0..4 {
            for _ in 0..6 {
                tracker.record("tail", t(minute), environment, Some(-60));
                minute += 15;
            }
        }
        let score = tracker.score("tail").unwrap();
        assert_eq!(score.distinct_environments, 4);
        assert!(
            score.classification >= Persistence::PossibleFollowing,
            "expected following behaviour, got {:?} at score {}",
            score.classification,
            score.score
        );
        assert!(
            score
                .reasons
                .iter()
                .any(|r| r.contains("4 distinct surroundings"))
        );
    }

    #[test]
    fn a_passing_device_is_incidental() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        for i in 0..3 {
            tracker.record("passer", t(i), 0, Some(-80));
        }
        let score = tracker.score("passer").unwrap();
        assert_eq!(score.classification, Persistence::Incidental);
    }

    #[test]
    fn reappearance_after_a_gap_is_counted() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        tracker.record("dev", t(0), 0, None);
        tracker.record("dev", t(1), 0, None);
        tracker.record("dev", t(120), 0, None); // long gap
        tracker.record("dev", t(121), 0, None);
        let score = tracker.score("dev").unwrap();
        assert_eq!(score.reappearances, 1);
        assert!(
            score
                .reasons
                .iter()
                .any(|r| r.contains("left and returned"))
        );
    }

    #[test]
    fn every_score_explains_itself() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        for i in 0..10 {
            tracker.record("dev", t(i * 20), i as u32 % 3, Some(-55 - i as i16));
        }
        let score = tracker.score("dev").unwrap();
        assert!(!score.reasons.is_empty());
        assert!(score.reasons.iter().any(|r| r.contains("seen 10 times")));
        assert!(score.reasons.iter().any(|r| r.contains("signal averaged")));
    }

    #[test]
    fn device_count_is_bounded_and_evicts_the_stalest() {
        let config = PersistenceConfig {
            max_tracked_devices: 3,
            ..PersistenceConfig::default()
        };
        let mut tracker = PersistenceTracker::new(config);
        tracker.record("old", t(0), 0, None);
        tracker.record("mid", t(10), 0, None);
        tracker.record("new", t(20), 0, None);
        assert_eq!(tracker.tracked_devices(), 3);

        tracker.record("newest", t(30), 0, None);
        assert_eq!(tracker.tracked_devices(), 3);
        // "old" had the oldest last_seen and should have gone.
        assert!(tracker.score("old").is_none());
    }

    #[test]
    fn sightings_per_device_are_bounded() {
        let config = PersistenceConfig {
            max_sightings_per_device: 10,
            ..PersistenceConfig::default()
        };
        let mut tracker = PersistenceTracker::new(config);
        for i in 0..500 {
            tracker.record("noisy", t(i), 0, Some(-50));
        }
        let score = tracker.score("noisy").unwrap();
        assert_eq!(score.sightings, 10);
    }

    #[test]
    fn pruning_forgets_old_sightings_and_empty_devices() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        for i in 0..5 {
            tracker.record("dev", t(i), 0, None);
        }
        assert_eq!(tracker.tracked_devices(), 1);
        tracker.prune(t(100));
        assert_eq!(tracker.tracked_devices(), 0);
    }

    #[test]
    fn above_returns_strongest_first() {
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        for i in 0..3 {
            tracker.record("weak", t(i), 0, None);
        }
        let mut minute = 0;
        for environment in 0..4 {
            for _ in 0..6 {
                tracker.record("strong", t(minute), environment, Some(-60));
                minute += 15;
            }
        }
        let ranked = tracker.above(Persistence::Persistent);
        assert!(!ranked.is_empty());
        assert_eq!(ranked[0].0, "strong");
    }

    #[test]
    fn the_tracker_never_needs_a_real_address() {
        // Keys are opaque: a session pseudonym works exactly as well as a MAC,
        // which is what lets persistence be scored without retaining
        // identifiers for devices that matched nothing.
        let mut tracker = PersistenceTracker::new(PersistenceConfig::default());
        for i in 0..6 {
            tracker.record("anon-6a591f42429f7454", t(i * 30), i as u32 % 2, Some(-70));
        }
        assert!(tracker.score("anon-6a591f42429f7454").is_some());
    }
}
