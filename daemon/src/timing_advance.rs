//! Watching timing advance for a cell that appears to have moved.
//!
//! Timing advance is the correction a tower tells a device to apply so its
//! transmissions land in the right slot, and it is proportional to the round
//! trip. It is the only distance measurement a phone gets for free, roughly 78
//! metres per step.
//!
//! The useful property is not the distance itself but that **a tower does not
//! move**. If the same cell identity is suddenly answering from a noticeably
//! different distance, either the device moved or something else is
//! transmitting that identity from somewhere else. The second is what a fake
//! base station looks like when it copies a real cell's identifiers, which is
//! the normal way of getting devices to attach to it.
//!
//! Two things keep this honest.
//!
//! **Devices move.** A hotspot in a bag produces exactly this signal all day,
//! so the threshold is generous and the wording never claims more than "this
//! changed". Movement is the likely explanation and the interface says so.
//!
//! **Not every modem reports it.** The Orbic RC400L returns zero on every
//! random access, verified against three real attaches and cross checked with
//! SCAT, so the field is simply not populated there rather than the tower being
//! 70 metres away. A detector that treats that as "always at zero distance,
//! never changing" would be silently dead, which is worse than absent. So an
//! all zero history reports as unsupported and says nothing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metres per timing advance step.
///
/// One step is 16 * Ts, about 0.52 microseconds of round trip. This measures
/// signal path, not map distance, so it reads high where the signal bounces.
pub const METRES_PER_STEP: f64 = 78.07;

/// How much a cell's distance must change before it is worth mentioning.
///
/// Deliberately generous. At roughly 78 metres a step this is about 1.2 km,
/// which a person walking will cross, so anything tighter would fire
/// constantly on a device that is carried around. What it still catches is the
/// large discontinuity of a different transmitter answering to the same
/// identity.
pub const JUMP_THRESHOLD_STEPS: u16 = 16;

/// Observations of one cell before its baseline is trusted.
///
/// A single random access can be a bad measurement. Two agreeing is enough to
/// call it a baseline without waiting so long that a short attachment never
/// produces one.
pub const SAMPLES_FOR_BASELINE: usize = 2;

/// The most cells tracked at once, so a device reselecting all day cannot grow
/// this without limit.
const MAX_CELLS: usize = 32;

/// How a cell's timing advance compares with what it reported before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum TimingAdvanceStatus {
    /// Nothing seen, or every value was zero. This modem does not report it.
    #[default]
    Unsupported,
    /// Seen, but not yet enough of this cell to have a baseline.
    Learning,
    /// Matches what this cell reported before.
    Consistent,
    /// This cell's distance changed by more than the threshold.
    Moved {
        /// What this cell used to report.
        baseline: u16,
        /// What it reports now.
        observed: u16,
        /// The change in metres, signed. Positive is further away.
        metres: i64,
    },
}

/// Rough distance implied by a timing advance value, in metres.
pub fn metres(ta: u16) -> i64 {
    (f64::from(ta) * METRES_PER_STEP).round() as i64
}

/// One cell's timing advance history.
#[derive(Debug, Clone, Default)]
struct CellSamples {
    /// The established value for this cell, once there is one.
    baseline: Option<u16>,
    /// Observations counted towards establishing it.
    seen: usize,
}

/// Timing advance seen per cell, and what it implies.
#[derive(Debug, Clone, Default)]
pub struct TimingAdvanceTracker {
    cells: HashMap<(u16, u32), CellSamples>,
    /// Whether any non zero value has ever been seen.
    ///
    /// The guard against a modem that reports zero for everything. Without it
    /// every cell would look permanently consistent at zero distance and the
    /// detector would appear to be working while detecting nothing.
    any_non_zero: bool,
    status: TimingAdvanceStatus,
}

impl TimingAdvanceTracker {
    /// Record a timing advance for a cell, and return what it implies.
    pub fn observe(&mut self, pci: u16, earfcn: u32, ta: u16) -> TimingAdvanceStatus {
        if ta != 0 {
            self.any_non_zero = true;
        }

        // Until something non zero turns up there is no evidence this modem
        // populates the field at all, so nothing is claimed either way.
        if !self.any_non_zero {
            self.status = TimingAdvanceStatus::Unsupported;
            return self.status;
        }

        // Bound the map. Dropping the whole set is crude but this is only a
        // baseline: it is rebuilt within a couple of observations, and the
        // alternative is tracking recency for something that rarely fills.
        if self.cells.len() >= MAX_CELLS && !self.cells.contains_key(&(pci, earfcn)) {
            self.cells.clear();
        }

        let entry = self.cells.entry((pci, earfcn)).or_default();
        entry.seen += 1;

        self.status = match entry.baseline {
            None => {
                if entry.seen >= SAMPLES_FOR_BASELINE {
                    entry.baseline = Some(ta);
                    TimingAdvanceStatus::Consistent
                } else {
                    TimingAdvanceStatus::Learning
                }
            }
            Some(baseline) => {
                let difference = ta.abs_diff(baseline);
                if difference >= JUMP_THRESHOLD_STEPS {
                    // Follow the cell to its new value, or every later
                    // observation from a device that genuinely moved would
                    // report the same jump for ever.
                    entry.baseline = Some(ta);
                    TimingAdvanceStatus::Moved {
                        baseline,
                        observed: ta,
                        metres: metres(ta) - metres(baseline),
                    }
                } else {
                    TimingAdvanceStatus::Consistent
                }
            }
        };
        self.status
    }

    /// The most recent verdict.
    pub fn status(&self) -> TimingAdvanceStatus {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard that matters most. An Orbic reports zero on every random
    /// access, and a detector that called that "consistent" would look alive
    /// while being incapable of ever firing.
    #[test]
    fn a_modem_reporting_only_zero_is_unsupported() {
        let mut t = TimingAdvanceTracker::default();
        for _ in 0..10 {
            assert_eq!(t.observe(427, 2125, 0), TimingAdvanceStatus::Unsupported);
        }
        assert_eq!(t.status(), TimingAdvanceStatus::Unsupported);
    }

    #[test]
    fn a_baseline_takes_a_couple_of_observations() {
        let mut t = TimingAdvanceTracker::default();
        assert_eq!(t.observe(1, 100, 40), TimingAdvanceStatus::Learning);
        assert_eq!(t.observe(1, 100, 40), TimingAdvanceStatus::Consistent);
    }

    #[test]
    fn small_changes_are_not_reported() {
        let mut t = TimingAdvanceTracker::default();
        t.observe(1, 100, 40);
        t.observe(1, 100, 40);
        // Well inside the threshold: ordinary movement or a noisy measurement.
        assert_eq!(t.observe(1, 100, 45), TimingAdvanceStatus::Consistent);
    }

    /// The signal this exists for: one identity, two distances.
    #[test]
    fn a_large_jump_for_the_same_cell_is_reported() {
        let mut t = TimingAdvanceTracker::default();
        t.observe(1, 100, 10);
        t.observe(1, 100, 10);
        let status = t.observe(1, 100, 90);
        match status {
            TimingAdvanceStatus::Moved {
                baseline,
                observed,
                metres,
            } => {
                assert_eq!(baseline, 10);
                assert_eq!(observed, 90);
                assert!(
                    metres > 6000,
                    "expected a large positive change, got {metres}"
                );
            }
            other => panic!("expected Moved, got {other:?}"),
        }
    }

    /// A device that really moved must not report the same jump for ever.
    #[test]
    fn the_baseline_follows_a_genuine_move() {
        let mut t = TimingAdvanceTracker::default();
        t.observe(1, 100, 10);
        t.observe(1, 100, 10);
        assert!(matches!(
            t.observe(1, 100, 90),
            TimingAdvanceStatus::Moved { .. }
        ));
        assert_eq!(t.observe(1, 100, 90), TimingAdvanceStatus::Consistent);
    }

    /// Cells are tracked separately, or reselecting between two towers at
    /// different distances would look like one tower jumping about.
    #[test]
    fn different_cells_do_not_contaminate_each_other() {
        let mut t = TimingAdvanceTracker::default();
        t.observe(1, 100, 10);
        t.observe(1, 100, 10);
        t.observe(2, 100, 90);
        assert_eq!(t.observe(2, 100, 90), TimingAdvanceStatus::Consistent);
        assert_eq!(t.observe(1, 100, 10), TimingAdvanceStatus::Consistent);
    }

    /// Same PCI on a different frequency is a different cell.
    #[test]
    fn earfcn_is_part_of_the_identity() {
        let mut t = TimingAdvanceTracker::default();
        t.observe(1, 100, 10);
        t.observe(1, 100, 10);
        assert_eq!(t.observe(1, 200, 90), TimingAdvanceStatus::Learning);
    }

    #[test]
    fn the_cell_map_stays_bounded() {
        let mut t = TimingAdvanceTracker::default();
        for pci in 0..(MAX_CELLS as u16 * 3) {
            t.observe(pci, 100, 20);
        }
        assert!(t.cells.len() <= MAX_CELLS, "grew to {}", t.cells.len());
    }

    #[test]
    fn distance_matches_the_published_step() {
        assert_eq!(metres(0), 0);
        assert_eq!(metres(1), 78);
        assert_eq!(metres(32), 2498);
    }
}
