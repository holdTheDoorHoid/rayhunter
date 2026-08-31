//! Whether the SIM appears to be working.
//!
//! A SIM that is dead, unactivated, or not seated looks very like a quiet
//! night: Rayhunter keeps running, the screen stays green, and nothing is
//! recorded, because nothing is happening. People have been told in the past
//! that an unactivated SIM "might work", which was never a satisfying answer.
//!
//! The distinction this draws is between hearing a network and talking to one.
//!
//! - A modem with **no usable SIM still sees towers**. It decodes the
//!   broadcasts every cell transmits, so a serving cell and neighbour
//!   measurements prove only that the radio works.
//! - **NAS is the conversation between this SIM and the network's core.** It
//!   does not happen without a SIM the network accepts, so NAS traffic is the
//!   evidence that separates the two cases.
//! - A **data bearer** is stronger still, but its absence proves nothing: some
//!   SIMs register happily and are given no data at all, which is why an
//!   earlier attempt at this check, based on having an IP, was abandoned.
//!
//! So the verdict leans on NAS, treats data as a bonus, and treats seeing
//! towers without ever registering as the thing worth warning about.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// How long without NAS traffic before a registered SIM stops counting as
/// proof of anything.
///
/// NAS is not continuous. Once attached and idle, a device can go a long time
/// without sending any, so this has to be generous or a working SIM gets
/// called broken every time the user stops using it. Attach, tracking area
/// updates and paging all land well inside an hour.
pub const NAS_FRESH_FOR_MINUTES: i64 = 60;

/// Long enough seeing towers without registering to be worth saying so.
///
/// Attaching takes seconds when it works. A few minutes of hearing towers and
/// never speaking to the core is not a slow attach, it is a SIM the network is
/// not accepting.
pub const SILENT_ATTACH_MINUTES: i64 = 5;

/// What the evidence adds up to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SimVerdict {
    /// Registered with the core and carrying data. Nothing to worry about.
    Working,
    /// Talking to the core, but no data bearer. Normal for many SIMs bought
    /// for this purpose, and fine for Rayhunter's own work.
    Registered,
    /// Towers are being heard but the SIM has never reached the core. This is
    /// what a dead, unactivated or badly seated SIM looks like.
    NotRegistering,
    /// Nothing heard yet. Too early, or no coverage.
    #[default]
    Searching,
}

impl SimVerdict {
    /// Whether this is worth drawing someone's attention to.
    ///
    /// Kept here rather than in the interface because the device's own display
    /// will need the same judgement, and two copies of it would drift.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_problem(self) -> bool {
        matches!(self, SimVerdict::NotRegistering)
    }
}

/// The evidence, and what it adds up to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct SimHealth {
    pub verdict: SimVerdict,
    /// A cellular interface carrying the default route, when there is one.
    pub data_interface: Option<String>,
    /// Whether NAS has been seen recently enough to count.
    pub nas_recent: bool,
    /// When NAS was last seen at all, in this run.
    #[cfg_attr(feature = "apidocs", schema(value_type = Option<String>))]
    pub last_nas_message: Option<DateTime<Local>>,
    /// Whether a serving cell is currently known.
    pub serving_cell: bool,
    /// How long towers have been heard without any NAS, in minutes. Only set
    /// while that is the situation.
    pub silent_for_minutes: Option<i64>,
}

/// Work out the verdict from the evidence.
///
/// Split out from everything that reads the system so it can be tested; the
/// judgement is the part worth getting right.
pub fn verdict(
    data_interface: Option<&str>,
    last_nas_message: Option<DateTime<Local>>,
    first_cell_seen: Option<DateTime<Local>>,
    serving_cell: bool,
    now: DateTime<Local>,
) -> (SimVerdict, bool, Option<i64>) {
    let nas_age = last_nas_message.map(|t| (now - t).num_minutes());
    let nas_recent = nas_age.is_some_and(|age| age <= NAS_FRESH_FOR_MINUTES);

    // Data implies registration, so it does not need NAS to have been seen in
    // this run. A device that attached before Rayhunter started is carrying
    // data without us having watched it happen.
    if data_interface.is_some() {
        return (SimVerdict::Working, nas_recent, None);
    }

    if nas_recent {
        return (SimVerdict::Registered, true, None);
    }

    // Hearing towers but never talking to the core. Only called a problem once
    // it has gone on long enough not to be an attach in progress.
    if serving_cell && last_nas_message.is_none() {
        let silent = first_cell_seen.map(|t| (now - t).num_minutes());
        if silent.is_some_and(|m| m >= SILENT_ATTACH_MINUTES) {
            return (SimVerdict::NotRegistering, false, silent);
        }
        return (SimVerdict::Searching, false, silent);
    }

    // NAS was seen once but has gone stale, with no data. Registered earlier
    // and quiet since is not evidence of a fault.
    if last_nas_message.is_some() {
        return (SimVerdict::Registered, false, None);
    }

    (SimVerdict::Searching, false, None)
}

/// The interface carrying the default route, when it is a cellular one.
///
/// Read from `/proc/net/route` rather than by asking the modem. Nothing is
/// sent to the modem, which matters here: every route that talks to the
/// modem's own control interfaces on this hardware was either refused or
/// capable of knocking the device off USB entirely.
pub fn cellular_default_route() -> Option<String> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in table.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let name = fields.next()?;
        let destination = fields.next()?;
        // A destination of all zeroes is the default route.
        if destination != "00000000" {
            continue;
        }
        if is_cellular_interface(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// Whether an interface name is the modem's data path rather than WiFi or
/// ethernet.
///
/// Name matching, because the alternative is asking the modem. `rmnet` is what
/// Qualcomm's data path is called on every device Rayhunter supports; the
/// others are here for the ones it may not have met yet.
pub fn is_cellular_interface(name: &str) -> bool {
    const PREFIXES: [&str; 4] = ["rmnet", "wwan", "rmnet_data", "ppp"];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Local> {
        Local::now()
    }

    #[test]
    fn data_means_working() {
        let (v, _, _) = verdict(Some("rmnet_data0"), None, None, true, now());
        assert_eq!(v, SimVerdict::Working);
    }

    /// Data proves registration even when Rayhunter did not watch the attach,
    /// which is the normal case since the modem attaches before it starts.
    #[test]
    fn data_without_seeing_nas_is_still_working() {
        let (v, _, _) = verdict(Some("rmnet_data0"), None, Some(now()), true, now());
        assert_eq!(v, SimVerdict::Working);
    }

    #[test]
    fn recent_nas_without_data_is_registered() {
        let n = now();
        let (v, recent, _) = verdict(None, Some(n - Duration::minutes(2)), None, true, n);
        assert_eq!(v, SimVerdict::Registered);
        assert!(recent);
    }

    /// The case this whole module exists for: towers heard, core never
    /// reached.
    #[test]
    fn towers_but_no_nas_for_long_enough_is_a_problem() {
        let n = now();
        let (v, _, silent) = verdict(None, None, Some(n - Duration::minutes(30)), true, n);
        assert_eq!(v, SimVerdict::NotRegistering);
        assert!(v.is_problem());
        assert_eq!(silent, Some(30));
    }

    /// An attach in progress must not be called a dead SIM.
    #[test]
    fn a_fresh_attach_is_not_yet_a_problem() {
        let n = now();
        let (v, _, _) = verdict(None, None, Some(n - Duration::minutes(1)), true, n);
        assert_eq!(v, SimVerdict::Searching);
        assert!(!v.is_problem());
    }

    /// A working SIM goes quiet when nobody is using it. That must not be
    /// reported as a fault.
    #[test]
    fn a_long_idle_after_registering_is_not_a_fault() {
        let n = now();
        let (v, recent, _) = verdict(None, Some(n - Duration::hours(9)), Some(n), true, n);
        assert_eq!(v, SimVerdict::Registered);
        assert!(!recent, "stale NAS should not read as fresh");
        assert!(!v.is_problem());
    }

    #[test]
    fn nothing_heard_is_searching() {
        let (v, _, _) = verdict(None, None, None, false, now());
        assert_eq!(v, SimVerdict::Searching);
    }

    #[test]
    fn cellular_interfaces_are_recognised() {
        assert!(is_cellular_interface("rmnet_data0"));
        assert!(is_cellular_interface("wwan0"));
        assert!(is_cellular_interface("ppp0"));
        assert!(!is_cellular_interface("wlan0"));
        assert!(!is_cellular_interface("bridge0"));
        assert!(!is_cellular_interface("lo"));
        assert!(!is_cellular_interface("eth0"));
    }
}
