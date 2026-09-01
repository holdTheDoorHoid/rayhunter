//! A cell that starts answering from a different distance.
//!
//! Timing advance is the correction a tower sends so a device's transmissions
//! land in the right slot. It is proportional to the round trip, roughly 78
//! metres a step, and it is the only distance measurement a device gets for
//! nothing.
//!
//! The useful property is not the distance but that **a tower does not move**.
//! A fake base station attracts devices by copying a real cell's identifiers,
//! but it cannot copy where that cell physically is. So the same cell identity
//! answering from a noticeably different distance means either this device
//! moved, or two different transmitters are using one identity.
//!
//! Two things keep this from crying wolf.
//!
//! **Devices move**, and a device carried around produces this signal
//! honestly. The threshold is deliberately about a kilometre, and the event
//! says movement explains it if you moved. It is a low severity observation,
//! not an accusation.
//!
//! **Not every modem reports timing advance.** The Orbic RC400L returns zero on
//! every random access, checked against three real attaches and cross checked
//! with SCAT's parser, so the field is unpopulated rather than the tower being
//! next door. Treating that as "always the same distance" would make this look
//! alive while being incapable of ever firing, so an all zero history is
//! treated as no data at all.

use std::borrow::Cow;
use std::collections::HashMap;

use telcom_parser::lte_rrc::{BCCH_DL_SCH_MessageType, BCCH_DL_SCH_MessageType_c1};

use super::analyzer::{Analyzer, Event, EventType};
use super::information_element::{InformationElement, LteInformationElement};
use deku::bitvec::*;

/// Metres per timing advance step: 16 * Ts, about 0.52 microseconds of round
/// trip. Signal path, not map distance, so it reads high where signals bounce.
pub const METRES_PER_STEP: f64 = 78.07;

/// How far a cell's distance must change before it is worth reporting.
///
/// About 1.2 km. Anything tighter fires constantly on a device someone is
/// carrying, which would train people to ignore it. What survives is the large
/// discontinuity of a different transmitter answering to the same identity.
pub const JUMP_THRESHOLD_STEPS: u16 = 16;

/// Observations of one cell before its value is treated as that cell's own.
///
/// A single random access can be a poor measurement, and reporting off one
/// sample would flag the noise rather than the cell.
const SAMPLES_FOR_BASELINE: usize = 2;

/// Most cells remembered at once, so a long drive cannot grow this without
/// limit.
const MAX_CELLS: usize = 32;

/// Rough distance implied by a timing advance value, in metres.
pub fn metres(ta: u16) -> i64 {
    (f64::from(ta) * METRES_PER_STEP).round() as i64
}

#[derive(Debug, Clone, Default)]
struct CellSamples {
    baseline: Option<u16>,
    seen: usize,
}

/// Watches for one cell identity reporting two different distances.
#[derive(Debug, Default)]
pub struct TimingAdvanceAnalyzer {
    /// The cell the device is currently camped on, from the most recent SIB1.
    current_cell: Option<u32>,
    cells: HashMap<u32, CellSamples>,
    /// Whether any non zero timing advance has ever been seen.
    ///
    /// Without this guard a modem that reports zero for everything looks like
    /// a device permanently at the same distance from every tower, and this
    /// analyzer would report "consistent" for ever while being unable to fire.
    any_non_zero: bool,
}

impl TimingAdvanceAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a timing advance against the current cell.
    ///
    /// Split from the message handling so the judgement can be tested without
    /// building RRC messages.
    fn observe(&mut self, ta: u16) -> Option<(u16, u16)> {
        if ta != 0 {
            self.any_non_zero = true;
        }
        // No evidence this modem populates the field, so claim nothing.
        if !self.any_non_zero {
            return None;
        }
        // Without knowing which cell answered, a change is meaningless: two
        // towers at different distances are not one tower that moved.
        let cell = self.current_cell?;

        if self.cells.len() >= MAX_CELLS && !self.cells.contains_key(&cell) {
            self.cells.clear();
        }
        let entry = self.cells.entry(cell).or_default();
        entry.seen += 1;

        match entry.baseline {
            None => {
                if entry.seen >= SAMPLES_FOR_BASELINE {
                    entry.baseline = Some(ta);
                }
                None
            }
            Some(baseline) => {
                if ta.abs_diff(baseline) >= JUMP_THRESHOLD_STEPS {
                    // Follow the cell to its new distance, or a device that
                    // genuinely moved reports the same jump for ever.
                    entry.baseline = Some(ta);
                    Some((baseline, ta))
                } else {
                    None
                }
            }
        }
    }
}

impl Analyzer for TimingAdvanceAnalyzer {
    fn get_name(&self) -> Cow<'_, str> {
        Cow::from("Cell answering from a different distance")
    }

    fn get_description(&self) -> Cow<'_, str> {
        Cow::from(
            "Compares the timing advance a cell reports against what the same cell reported before. A tower does not move, so a large change means either this device moved or something else is transmitting that cell's identity. Silent on modems that do not report timing advance.",
        )
    }

    fn get_version(&self) -> u32 {
        1
    }

    fn analyze_information_element(
        &mut self,
        ie: &InformationElement,
        _packet_num: usize,
    ) -> Option<Event> {
        let InformationElement::LTE(lte) = ie else {
            return None;
        };

        // Which cell we are camped on, so a distance can be attributed.
        if let LteInformationElement::BcchDlSch(sch_msg) = &**lte
            && let BCCH_DL_SCH_MessageType::C1(c1) = &sch_msg.message
            && let BCCH_DL_SCH_MessageType_c1::SystemInformationBlockType1(sib1) = c1
        {
            self.current_cell = Some(
                sib1.cell_access_related_info
                    .cell_identity
                    .0
                    .as_bitslice()
                    .load_be::<u32>(),
            );
            return None;
        }

        let LteInformationElement::MacRar(rar) = &**lte else {
            return None;
        };
        let (baseline, observed) = self.observe(rar.timing_advance)?;

        let change = metres(observed) - metres(baseline);
        let direction = if change > 0 { "further" } else { "closer" };
        Some(Event {
            event_type: EventType::Low,
            message: format!(
                "Cell reported timing advance {observed}, having reported {baseline} before: about {} metres {direction}. If this device moved, that explains it; if it did not, the same cell identity is being transmitted from somewhere else.",
                change.abs()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyzer_on_cell(cell: u32) -> TimingAdvanceAnalyzer {
        let mut a = TimingAdvanceAnalyzer::new();
        a.current_cell = Some(cell);
        a
    }

    /// The guard that matters most: an Orbic reports zero on every attach, and
    /// this must produce nothing at all rather than a stream of "consistent".
    #[test]
    fn a_modem_reporting_only_zero_never_fires() {
        let mut a = analyzer_on_cell(1);
        for _ in 0..20 {
            assert_eq!(a.observe(0), None);
        }
    }

    #[test]
    fn a_baseline_takes_more_than_one_sample() {
        let mut a = analyzer_on_cell(1);
        assert_eq!(a.observe(40), None);
        assert_eq!(a.observe(40), None);
        // Only now is there something to compare against.
        assert_eq!(a.observe(90), Some((40, 90)));
    }

    #[test]
    fn small_changes_are_ignored() {
        let mut a = analyzer_on_cell(1);
        a.observe(40);
        a.observe(40);
        assert_eq!(a.observe(48), None);
    }

    /// A device that really moved must not report the same jump for ever.
    #[test]
    fn the_baseline_follows_a_genuine_move() {
        let mut a = analyzer_on_cell(1);
        a.observe(10);
        a.observe(10);
        assert_eq!(a.observe(90), Some((10, 90)));
        assert_eq!(a.observe(90), None);
    }

    /// Reselecting between two towers at different distances is not one tower
    /// moving.
    #[test]
    fn cells_are_tracked_separately() {
        let mut a = analyzer_on_cell(1);
        a.observe(10);
        a.observe(10);
        a.current_cell = Some(2);
        a.observe(90);
        assert_eq!(a.observe(90), None);
        a.current_cell = Some(1);
        assert_eq!(a.observe(10), None);
    }

    /// Without knowing the cell there is nothing to compare against, so a
    /// distance seen before any SIB1 must not be attributed to anything.
    #[test]
    fn nothing_is_claimed_before_the_cell_is_known() {
        let mut a = TimingAdvanceAnalyzer::new();
        assert_eq!(a.observe(40), None);
        assert_eq!(a.observe(90), None);
    }

    #[test]
    fn the_cell_map_stays_bounded() {
        let mut a = TimingAdvanceAnalyzer::new();
        for cell in 0..(MAX_CELLS as u32 * 3) {
            a.current_cell = Some(cell);
            a.observe(20);
        }
        assert!(a.cells.len() <= MAX_CELLS, "grew to {}", a.cells.len());
    }

    #[test]
    fn distance_matches_the_published_step() {
        assert_eq!(metres(0), 0);
        assert_eq!(metres(1), 78);
        assert_eq!(metres(32), 2498);
    }
}
