use rayhunter::analysis::analyzer::EventType;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub mod generic_framebuffer;

pub mod headless;
pub mod orbic;
pub mod tmobile;
pub mod tplink;
pub mod tplink_framebuffer;
pub mod tplink_onebit;
pub mod uz801;
pub mod wingtech;

/// A list of available display states
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum DisplayState {
    /// We're recording but no warning has been found yet.
    Recording,
    /// We're not recording.
    Paused,
    /// A non-informational event has been detected.
    ///
    /// Note that EventType::Informational is never sent through this. If it is, it's the same as
    /// Recording
    WarningDetected { event_type: EventType },
}

/// How long the device's own screen is left alone after a button press.
///
/// Long enough to read a wifi password off the screen and type it somewhere,
/// short enough that a detector is not blind for meaningfully long. The status
/// line comes straight back if a warning arrives, so this only delays the
/// display, never the detection.
pub const KEYPRESS_QUIET_PERIOD: Duration = Duration::from_secs(20);

/// A request to stop drawing over the device's own screen for a moment.
///
/// Rayhunter paints on top of the manufacturer's interface. In the modes that
/// fill the screen, a custom image or high visibility, that interface is
/// completely hidden, including the pages showing the wifi name and password.
/// Somebody who has not written the password down and cannot reach it in the
/// device's menus has locked themselves out of their own hotspot, which is a
/// bad outcome for a change they made to the colour of a status light.
///
/// So a button press, which is how a person navigates that interface, buys a
/// short window with Rayhunter's drawing held back.
#[derive(Debug)]
pub struct DisplaySuppression {
    /// Milliseconds since `base` until which drawing is held back. Zero means
    /// nothing is being held back.
    until_ms: AtomicU64,
    base: Instant,
}

impl DisplaySuppression {
    pub fn new() -> Self {
        Self {
            until_ms: AtomicU64::new(0),
            base: Instant::now(),
        }
    }

    /// Hold drawing back for `period` from now.
    ///
    /// Later presses extend the window rather than shortening it, so somebody
    /// pressing buttons repeatedly keeps the screen rather than losing it
    /// halfway through reading something.
    pub fn suppress_for(&self, period: Duration) {
        let deadline = (self.base.elapsed() + period).as_millis() as u64;
        self.until_ms.fetch_max(deadline, Ordering::Relaxed);
    }

    /// Whether drawing should be held back right now.
    pub fn active(&self) -> bool {
        let until = self.until_ms.load(Ordering::Relaxed);
        until != 0 && (self.base.elapsed().as_millis() as u64) < until
    }
}

impl Default for DisplaySuppression {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle to the suppression state.
pub type SharedSuppression = Arc<DisplaySuppression>;

#[cfg(test)]
mod suppression_tests {
    use super::*;

    #[test]
    fn nothing_is_suppressed_to_begin_with() {
        let s = DisplaySuppression::new();
        assert!(!s.active());
    }

    #[test]
    fn a_press_holds_drawing_back() {
        let s = DisplaySuppression::new();
        s.suppress_for(Duration::from_secs(30));
        assert!(s.active());
    }

    /// A press while the screen is already held must not shorten the window.
    /// Somebody pressing buttons in a menu would otherwise lose the screen
    /// halfway through reading it, which is the opposite of the point.
    #[test]
    fn a_later_press_extends_rather_than_replaces() {
        let s = DisplaySuppression::new();
        s.suppress_for(Duration::from_secs(60));
        let long = s.until_ms.load(Ordering::Relaxed);
        s.suppress_for(Duration::from_millis(1));
        assert_eq!(s.until_ms.load(Ordering::Relaxed), long);
        assert!(s.active());
    }

    #[test]
    fn an_elapsed_window_stops_being_active() {
        let s = DisplaySuppression::new();
        s.suppress_for(Duration::from_millis(0));
        std::thread::sleep(Duration::from_millis(5));
        assert!(!s.active());
    }
}
