//! Turning ADB on or off from the settings page.
//!
//! Some devices choose their USB composition at boot from `/usrdata/mode.cfg`,
//! and one of the choices includes ADB. Setting it there is what makes ADB
//! survive a restart, where the composition command on its own does not.
//!
//! **The value is device specific, and that is the whole difficulty.** Measured
//! here: a Moxee K779HSDL uses `9` for the composition with ADB and `3` for the
//! one without, while an Orbic RC400L sitting next to it has `1` in the same
//! file for a composition that also has ADB. The file is identical, the numbers
//! are not. Writing a Moxee's value to an Orbic would select something nobody
//! has checked, and getting a USB composition wrong takes the device off USB
//! entirely, which is the one failure that needs a cable to fix.
//!
//! So this only ever changes a value it recognises. A file holding `3` or `9`
//! is a mapping that has been verified on hardware and can be moved between
//! those two. Anything else, including the Orbic's `1`, is left exactly as it
//! is and reported as not adjustable. Devices that already have ADB from their
//! installer keep it, which is the intent: this adds a way to turn it on, not a
//! way to have it taken away.

use serde::{Deserialize, Serialize};

/// Selects the USB composition at boot, on the devices that use it.
pub const MODE_FILE: &str = "/usrdata/mode.cfg";

/// The composition including ADB, on a Moxee. Verified on hardware.
pub const MODE_WITH_ADB: &str = "9";

/// RNDIS only, no ADB, on a Moxee. Verified on hardware.
pub const MODE_WITHOUT_ADB: &str = "3";

/// What can be said about ADB on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum AdbState {
    /// Recognised, and currently on.
    Enabled,
    /// Recognised, and currently off.
    Disabled,
    /// This device chooses its composition some other way, or by a value
    /// nobody has verified. Whatever ADB it has is left alone.
    NotAdjustable,
}

/// Read the mode file and say what it means, if anything.
///
/// Split from any file handling so the judgement can be tested. It is the
/// judgement that matters: the cost of getting it wrong is a device that no
/// longer appears on USB.
pub fn interpret(contents: Option<&str>) -> AdbState {
    match contents.map(str::trim) {
        Some(MODE_WITH_ADB) => AdbState::Enabled,
        Some(MODE_WITHOUT_ADB) => AdbState::Disabled,
        _ => AdbState::NotAdjustable,
    }
}

/// The value to write for a wanted state, or `None` when this device's mode
/// file is not one we recognise.
///
/// Returns `None` rather than a best guess. There is no safe guess here.
pub fn value_for(current: AdbState, enabled: bool) -> Option<&'static str> {
    match current {
        AdbState::NotAdjustable => None,
        _ if enabled => Some(MODE_WITH_ADB),
        _ => Some(MODE_WITHOUT_ADB),
    }
}

/// What this device's mode file says now.
pub fn current_state() -> AdbState {
    let contents = std::fs::read_to_string(MODE_FILE).ok();
    interpret(contents.as_deref())
}

/// Make ADB match the wanted state, if this device is one we can adjust.
///
/// Returns whether anything was written. Takes effect at the next boot, since
/// the composition is chosen then; the caller is expected to say so rather than
/// implying the change is immediate.
pub fn apply(enabled: bool) -> Result<bool, String> {
    let current = current_state();
    let Some(wanted) = value_for(current, enabled) else {
        return Err(format!(
            "{MODE_FILE} does not hold a value this knows how to change; leaving it alone"
        ));
    };

    let already = match current {
        AdbState::Enabled => enabled,
        AdbState::Disabled => !enabled,
        AdbState::NotAdjustable => unreachable!("value_for returned None for NotAdjustable"),
    };
    if already {
        return Ok(false);
    }

    std::fs::write(MODE_FILE, format!("{wanted}\n"))
        .map_err(|err| format!("could not write {MODE_FILE}: {err}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_verified_values_are_understood() {
        assert_eq!(interpret(Some("9")), AdbState::Enabled);
        assert_eq!(interpret(Some("3")), AdbState::Disabled);
        // Trailing newline is how the file actually reads.
        assert_eq!(interpret(Some("9\n")), AdbState::Enabled);
    }

    /// The Orbic sitting next to the Moxee has `1` in this same file, for a
    /// composition that also has ADB. Treating that as something to change
    /// would write a Moxee's number onto an Orbic and select a composition
    /// nobody has checked.
    #[test]
    fn an_orbics_value_is_left_alone() {
        assert_eq!(interpret(Some("1")), AdbState::NotAdjustable);
        assert_eq!(value_for(AdbState::NotAdjustable, true), None);
        assert_eq!(value_for(AdbState::NotAdjustable, false), None);
    }

    #[test]
    fn a_missing_or_junk_file_is_left_alone() {
        for contents in [None, Some(""), Some("banana"), Some("99"), Some("0")] {
            assert_eq!(
                interpret(contents),
                AdbState::NotAdjustable,
                "for {contents:?}"
            );
        }
    }

    #[test]
    fn the_wanted_value_is_the_verified_one() {
        assert_eq!(value_for(AdbState::Disabled, true), Some("9"));
        assert_eq!(value_for(AdbState::Enabled, false), Some("3"));
        // Asking for what it already is still names the same value.
        assert_eq!(value_for(AdbState::Enabled, true), Some("9"));
    }
}
