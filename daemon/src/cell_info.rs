//! Tracks what the modem currently sees on the air, so the web UI can show it.
//!
//! Rayhunter already decodes serving cell and neighbour measurements (added in
//! upstream PR #1074) but nothing kept them, so they were parsed and dropped.
//! This retains the most recent view plus a bounded history of the cells seen
//! during a run. See upstream issue #326.
//!
//! # A warning about identity
//!
//! There are two very different kinds of "which cell is this" here, and
//! conflating them would mislead people:
//!
//! - The **serving cell** broadcasts a globally unique identity in SIB1, so a
//!   cell we actually attach to can be named properly.
//! - **Neighbours** are only ever reported by *physical cell identity*, which
//!   has 504 possible values and is reused constantly across a network. It
//!   distinguishes signals locally on one frequency. It is not a tower
//!   identity, and two unrelated towers can share one.
//!
//! Everything below keeps those separate, and the API never presents a
//! neighbour as though it were an identified tower.

use std::collections::VecDeque;

use chrono::{DateTime, Local};
use rayhunter::analysis::information_element::{InformationElement, LteInformationElement};
use rayhunter::telcom_parser::lte_rrc::{BCCH_DL_SCH_MessageType, BCCH_DL_SCH_MessageType_c1};
use serde::{Deserialize, Serialize};

/// How many past observations to keep. Each is small, and the device has very
/// little RAM free, so this is deliberately modest.
const HISTORY_LIMIT: usize = 256;

/// A globally unique cell identity, only knowable for a cell we attach to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct CellIdentity {
    /// Mobile Country Code, e.g. "310" for the United States. Kept as digits
    /// rather than a number because leading zeros are significant.
    pub mcc: Option<String>,
    /// Mobile Network Code identifying the operator within that country.
    ///
    /// Also digits, and for a stronger reason: an MNC may be two or three
    /// digits and the length is part of the identity, so "30" and "030" are
    /// different networks. Storing a number would silently merge them.
    pub mnc: Option<String>,
    /// The 28 bit cell identity from SIB1. Globally unique within the operator.
    pub cell_id: Option<u32>,
    /// Tracking area code, the grouping the network uses to page your phone.
    pub tac: Option<u32>,
}

/// Signal measurements, all in the units the radio reports them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct SignalMeasurements {
    /// Reference Signal Received Power, in dBm. Roughly minus 80 is strong and
    /// minus 110 is weak.
    pub rsrp_dbm: f32,
    /// Reference Signal Received Quality, in dB. Higher is cleaner.
    pub rsrq_db: f32,
    /// Total received power including noise and interference, in dBm.
    pub rssi_dbm: f32,
}

/// The cell currently serving this device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct ServingCell {
    /// Physical cell identity. Local to a frequency, not globally unique.
    pub pci: u16,
    /// The frequency channel number this cell is on.
    pub earfcn: u32,
    /// Band derived from the EARFCN, when it falls in a known range.
    pub band: Option<u16>,
    pub signal: SignalMeasurements,
    /// Present only once the cell's SIB1 broadcast has been seen.
    pub identity: Option<CellIdentity>,
    /// Raw timing advance from the most recent random access response.
    ///
    /// Each step is about 0.52 microseconds of round trip, so roughly 78 metres
    /// of separation. It only arrives when the device performs random access,
    /// which is not continuous, so this is the last value seen rather than a
    /// live one.
    pub timing_advance: Option<u16>,
    pub last_seen: DateTime<Local>,
}

/// A neighbouring cell the modem can hear but is not attached to.
///
/// Deliberately has no identity field. All we get is a physical cell identity,
/// which does not name a tower.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct NeighborCell {
    pub pci: u16,
    pub earfcn: u32,
    pub signal: SignalMeasurements,
}

/// One entry in the record of cells seen during this run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct CellObservation {
    pub pci: u16,
    pub earfcn: u32,
    pub identity: Option<CellIdentity>,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
    /// Strongest RSRP seen while attached to this cell, in dBm.
    pub best_rsrp_dbm: f32,
}

/// Everything the UI needs in one response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct CellInfo {
    pub serving: Option<ServingCell>,
    pub neighbors: Vec<NeighborCell>,
    pub history: Vec<CellObservation>,
    /// False when no measurement has arrived yet, which is the normal state
    /// while recording is stopped. Lets the UI explain the emptiness rather
    /// than looking broken.
    pub has_data: bool,
}

/// Accumulates measurements as diag messages are parsed.
#[derive(Debug, Default)]
pub struct CellTracker {
    serving: Option<ServingCell>,
    neighbors: Vec<NeighborCell>,
    history: VecDeque<CellObservation>,
}

/// Map an EARFCN to its LTE band, for the FDD downlink ranges.
///
/// Only covers the common bands; an unknown value returns None rather than a
/// guess, since a wrong band is worse than no band for someone doing research.
pub fn band_for_earfcn(earfcn: u32) -> Option<u16> {
    const RANGES: &[(u32, u32, u16)] = &[
        (0, 599, 1),
        (600, 1199, 2),
        (1200, 1949, 3),
        (1950, 2399, 4),
        (2400, 2649, 5),
        (2750, 3449, 7),
        (3450, 3799, 8),
        (3800, 4149, 9),
        (4150, 4749, 10),
        (5010, 5179, 12),
        (5180, 5279, 13),
        (5280, 5379, 14),
        (5730, 5849, 17),
        (5850, 5999, 18),
        (6000, 6149, 19),
        (6150, 6449, 20),
        (6450, 6599, 21),
        (7500, 7699, 24),
        (7700, 8039, 25),
        (8040, 8689, 26),
        (8690, 9039, 27),
        (9040, 9209, 28),
        (9210, 9659, 29),
        (9770, 9869, 30),
        (9870, 9919, 31),
        (36000, 36199, 33),
        (36200, 36349, 34),
        (36350, 36949, 35),
        (36950, 37549, 36),
        (37550, 37749, 37),
        (37750, 38249, 38),
        (38250, 38649, 39),
        (38650, 39649, 40),
        (39650, 41589, 41),
        (41590, 43589, 42),
        (43590, 45589, 43),
        (45590, 46589, 44),
        (46590, 46789, 45),
        (46790, 54539, 46),
        (55240, 56739, 48),
        (56740, 58239, 49),
        (58240, 59089, 50),
        (59090, 59139, 51),
        (59140, 60139, 52),
        (60140, 60254, 53),
        (65536, 66435, 65),
        (66436, 67335, 66),
        (67336, 67535, 67),
        (67536, 67835, 68),
        (67836, 68335, 69),
        (68336, 68585, 70),
        (68586, 68935, 71),
    ];
    RANGES
        .iter()
        .find(|(lo, hi, _)| earfcn >= *lo && earfcn <= *hi)
        .map(|(_, _, band)| *band)
}

impl CellTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a serving cell measurement.
    pub fn update_serving(&mut self, pci: u16, earfcn: u32, signal: SignalMeasurements) {
        let now = Local::now();

        // Carry the identity across measurements of the same cell, since it
        // arrives on a broadcast that is far less frequent than measurements.
        let identity = match &self.serving {
            Some(prev) if prev.pci == pci && prev.earfcn == earfcn => prev.identity.clone(),
            _ => None,
        };

        // Timing advance survives a measurement of the same cell, since random
        // access happens far less often than measurement.
        let timing_advance = match &self.serving {
            Some(prev) if prev.pci == pci && prev.earfcn == earfcn => prev.timing_advance,
            _ => None,
        };

        self.serving = Some(ServingCell {
            pci,
            earfcn,
            band: band_for_earfcn(earfcn),
            signal,
            identity,
            timing_advance,
            last_seen: now,
        });
        self.record_observation(pci, earfcn, signal.rsrp_dbm, now);
    }

    /// Record a timing advance from a random access response.
    pub fn update_timing_advance(&mut self, ta: u16) {
        if let Some(serving) = self.serving.as_mut() {
            serving.timing_advance = Some(ta);
        }
    }

    /// Attach a decoded identity to the current serving cell.
    pub fn update_identity(&mut self, identity: CellIdentity) {
        let Some(serving) = self.serving.as_mut() else {
            return;
        };
        serving.identity = Some(identity.clone());
        if let Some(entry) = self
            .history
            .iter_mut()
            .rev()
            .find(|e| e.pci == serving.pci && e.earfcn == serving.earfcn)
        {
            entry.identity = Some(identity);
        }
    }

    /// Replace the neighbour list. Neighbours are a snapshot rather than an
    /// accumulation: a cell that has dropped out of range should disappear.
    pub fn update_neighbors(&mut self, neighbors: Vec<NeighborCell>) {
        self.neighbors = neighbors;
    }

    fn record_observation(&mut self, pci: u16, earfcn: u32, rsrp: f32, now: DateTime<Local>) {
        if let Some(existing) = self
            .history
            .iter_mut()
            .find(|e| e.pci == pci && e.earfcn == earfcn)
        {
            existing.last_seen = now;
            if rsrp > existing.best_rsrp_dbm {
                existing.best_rsrp_dbm = rsrp;
            }
            return;
        }

        if self.history.len() >= HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.history.push_back(CellObservation {
            pci,
            earfcn,
            identity: None,
            first_seen: now,
            last_seen: now,
            best_rsrp_dbm: rsrp,
        });
    }

    pub fn snapshot(&self) -> CellInfo {
        let mut history: Vec<_> = self.history.iter().cloned().collect();
        // Most recently seen first, which is the order people look for.
        history.sort_by_key(|e| std::cmp::Reverse(e.last_seen));

        let mut neighbors = self.neighbors.clone();
        // Strongest first, so the most likely reselection target is at the top.
        neighbors.sort_by(|a, b| {
            b.signal
                .rsrp_dbm
                .partial_cmp(&a.signal.rsrp_dbm)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        CellInfo {
            has_data: self.serving.is_some() || !history.is_empty(),
            serving: self.serving.clone(),
            neighbors,
            history,
        }
    }
}

/// One decimal digit as a character. Values outside 0 to 9 cannot occur in a
/// well formed PLMN, and become '?' rather than nonsense.
fn digit_char(value: u8) -> char {
    if value < 10 {
        (b'0' + value) as char
    } else {
        '?'
    }
}

/// Pull the globally unique identity out of a tower's SIB1 broadcast.
///
/// SIB1 is the only place this is available, and only for a cell the device
/// actually attaches to, which is why neighbours can never be identified this
/// way. Returns None for any other message.
pub fn identity_from_information_element(ie: &InformationElement) -> Option<CellIdentity> {
    let InformationElement::LTE(lte) = ie else {
        return None;
    };
    let LteInformationElement::BcchDlSch(sch) = &**lte else {
        return None;
    };
    let BCCH_DL_SCH_MessageType::C1(c1) = &sch.message else {
        return None;
    };
    let BCCH_DL_SCH_MessageType_c1::SystemInformationBlockType1(sib1) = c1 else {
        return None;
    };

    let info = &sib1.cell_access_related_info;
    // SIB1 packs these as bit vectors of non byte aligned width (28 bits for
    // the cell identity, 16 for the tracking area), so they are folded a bit at
    // a time rather than read as an integer.
    let cell_id = info
        .cell_identity
        .0
        .iter()
        .fold(0u32, |acc, bit| (acc << 1) | (*bit as u32));
    let tac = info
        .tracking_area_code
        .0
        .iter()
        .fold(0u32, |acc, bit| (acc << 1) | (*bit as u32));

    // MCC is always three digits; MNC is two or three, and the length is
    // meaningful, so both are folded into a single number the same way they are
    // written down.
    let plmn = info.plmn_identity_list.0.first().map(|p| &p.plmn_identity);
    let mcc = plmn
        .and_then(|p| p.mcc.as_ref())
        .map(|mcc| mcc.0.iter().map(|d| digit_char(d.0)).collect::<String>());
    let mnc = plmn.map(|p| p.mnc.0.iter().map(|d| digit_char(d.0)).collect::<String>());

    Some(CellIdentity {
        mcc,
        mnc,
        cell_id: Some(cell_id),
        tac: Some(tac),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(rsrp: f32) -> SignalMeasurements {
        SignalMeasurements {
            rsrp_dbm: rsrp,
            rsrq_db: -10.0,
            rssi_dbm: -60.0,
        }
    }

    #[test]
    fn starts_empty_so_the_ui_can_explain_itself() {
        let snap = CellTracker::new().snapshot();
        assert!(!snap.has_data);
        assert!(snap.serving.is_none());
        assert!(snap.neighbors.is_empty());
    }

    #[test]
    fn tracks_the_serving_cell() {
        let mut t = CellTracker::new();
        t.update_serving(160, 2050, sig(-85.0));
        let snap = t.snapshot();
        let serving = snap.serving.unwrap();
        assert_eq!(serving.pci, 160);
        assert_eq!(serving.earfcn, 2050);
        assert_eq!(serving.band, Some(4));
        assert!(snap.has_data);
    }

    /// The identity arrives on a broadcast far less often than measurements do,
    /// so it must survive them rather than being cleared each time.
    #[test]
    fn identity_survives_later_measurements_of_the_same_cell() {
        let mut t = CellTracker::new();
        t.update_serving(160, 2050, sig(-85.0));
        t.update_identity(CellIdentity {
            mcc: Some("310".into()),
            mnc: Some("260".into()),
            cell_id: Some(0x1234567),
            tac: Some(42),
        });
        t.update_serving(160, 2050, sig(-83.0));

        let identity = t.snapshot().serving.unwrap().identity.unwrap();
        assert_eq!(identity.mcc.as_deref(), Some("310"));
        assert_eq!(identity.cell_id, Some(0x1234567));
    }

    /// Moving to a different cell must not carry the old cell's identity over,
    /// which would label the new tower with someone else's name.
    #[test]
    fn identity_is_dropped_when_the_cell_changes() {
        let mut t = CellTracker::new();
        t.update_serving(160, 2050, sig(-85.0));
        t.update_identity(CellIdentity {
            mcc: Some("310".into()),
            mnc: Some("260".into()),
            cell_id: Some(1),
            tac: Some(42),
        });
        t.update_serving(200, 2050, sig(-90.0));
        assert!(t.snapshot().serving.unwrap().identity.is_none());
    }

    #[test]
    fn history_accumulates_distinct_cells_and_keeps_the_best_signal() {
        let mut t = CellTracker::new();
        t.update_serving(160, 2050, sig(-95.0));
        t.update_serving(160, 2050, sig(-80.0));
        t.update_serving(200, 2050, sig(-100.0));

        let history = t.snapshot().history;
        assert_eq!(history.len(), 2);
        let first = history.iter().find(|e| e.pci == 160).unwrap();
        assert_eq!(first.best_rsrp_dbm, -80.0);
    }

    #[test]
    fn history_is_bounded() {
        let mut t = CellTracker::new();
        for pci in 0..(HISTORY_LIMIT + 50) {
            t.update_serving(pci as u16, 2050, sig(-90.0));
        }
        assert_eq!(t.snapshot().history.len(), HISTORY_LIMIT);
    }

    /// Neighbours come and go, so the list is a snapshot rather than a running
    /// total. A cell out of range should stop being listed.
    #[test]
    fn neighbors_replace_rather_than_accumulate() {
        let mut t = CellTracker::new();
        t.update_neighbors(vec![NeighborCell {
            pci: 1,
            earfcn: 2050,
            signal: sig(-90.0),
        }]);
        t.update_neighbors(vec![NeighborCell {
            pci: 2,
            earfcn: 2050,
            signal: sig(-95.0),
        }]);
        let n = t.snapshot().neighbors;
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].pci, 2);
    }

    #[test]
    fn neighbors_are_sorted_strongest_first() {
        let mut t = CellTracker::new();
        t.update_neighbors(vec![
            NeighborCell {
                pci: 1,
                earfcn: 2050,
                signal: sig(-110.0),
            },
            NeighborCell {
                pci: 2,
                earfcn: 2050,
                signal: sig(-75.0),
            },
            NeighborCell {
                pci: 3,
                earfcn: 2050,
                signal: sig(-95.0),
            },
        ]);
        let pcis: Vec<_> = t.snapshot().neighbors.iter().map(|n| n.pci).collect();
        assert_eq!(pcis, vec![2, 3, 1]);
    }

    #[test]
    fn maps_earfcns_to_bands_and_admits_when_it_cannot() {
        assert_eq!(band_for_earfcn(2050), Some(4));
        assert_eq!(band_for_earfcn(5230), Some(13));
        assert_eq!(band_for_earfcn(66436), Some(66));
        // A gap between defined ranges must not be guessed at.
        assert_eq!(band_for_earfcn(2700), None);
        assert_eq!(band_for_earfcn(999_999), None);
    }
}
