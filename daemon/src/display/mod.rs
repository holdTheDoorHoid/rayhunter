use rayhunter::analysis::analyzer::EventType;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub mod generic_framebuffer;

pub mod headless;
pub mod orbic;
pub mod qr;
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

/// A picture that takes the whole screen for a while.
///
/// Used for the pairing code a new unit shows on first boot, and for anything
/// else that has to be on screen regardless of what Rayhunter would normally
/// be drawing: the picture replaces the status line, the logo, the custom
/// image, all of it, until its time is up or it is cleared. The display loop
/// asks for it on every pass and paints it fresh each time, since the
/// device's own interface keeps writing over parts of the framebuffer.
///
/// A button press still hands the screen back for a moment, exactly as it
/// does for a custom image. The person pressing buttons on the device is
/// usually trying to read the WiFi password off it, and during setup that
/// is precisely what they need to be able to do.
#[derive(Debug, Default)]
pub struct DisplayOverride {
    inner: Mutex<Option<OverrideFrame>>,
}

#[derive(Debug)]
struct OverrideFrame {
    pixels: Arc<Vec<(u8, u8, u8)>>,
    until: Instant,
}

impl DisplayOverride {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show `pixels`, a full screen's worth, for `period` from now.
    ///
    /// Replaces whatever was showing before; the newest request wins.
    pub fn show(&self, pixels: Vec<(u8, u8, u8)>, period: Duration) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *inner = Some(OverrideFrame {
            pixels: Arc::new(pixels),
            until: Instant::now() + period,
        });
    }

    /// Take the picture down early.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *inner = None;
    }

    /// The picture to paint right now, or `None` if there is none or it has
    /// expired. An expired one is dropped on the way out so its pixels are
    /// not held for nothing.
    pub fn current(&self) -> Option<Arc<Vec<(u8, u8, u8)>>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.as_ref() {
            Some(frame) if Instant::now() < frame.until => Some(frame.pixels.clone()),
            Some(_) => {
                *inner = None;
                None
            }
            None => None,
        }
    }

    /// Whether a picture is up.
    pub fn active(&self) -> bool {
        self.current().is_some()
    }

    /// How much longer the current picture has, if there is one.
    pub fn remaining(&self) -> Option<Duration> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .as_ref()
            .and_then(|f| f.until.checked_duration_since(Instant::now()))
    }
}

/// Shared handle to the override.
pub type SharedOverride = Arc<DisplayOverride>;

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

#[cfg(test)]
mod override_tests {
    use super::*;

    #[test]
    fn nothing_is_showing_to_begin_with() {
        let o = DisplayOverride::new();
        assert!(!o.active());
        assert!(o.current().is_none());
        assert!(o.remaining().is_none());
    }

    #[test]
    fn a_picture_shows_until_cleared() {
        let o = DisplayOverride::new();
        o.show(vec![(1, 2, 3); 4], Duration::from_secs(60));
        assert!(o.active());
        assert_eq!(o.current().unwrap().len(), 4);
        assert!(o.remaining().unwrap() > Duration::from_secs(50));
        o.clear();
        assert!(!o.active());
    }

    /// An expired picture must come down on its own. Setup mode is a window,
    /// not a permanent state, and nobody is around to clear it.
    #[test]
    fn a_picture_comes_down_when_its_time_is_up() {
        let o = DisplayOverride::new();
        o.show(vec![(0, 0, 0); 4], Duration::from_millis(0));
        std::thread::sleep(Duration::from_millis(5));
        assert!(o.current().is_none());
        assert!(!o.active());
    }

    #[test]
    fn the_newest_picture_wins() {
        let o = DisplayOverride::new();
        o.show(vec![(1, 1, 1); 4], Duration::from_secs(60));
        o.show(vec![(2, 2, 2); 9], Duration::from_secs(60));
        assert_eq!(o.current().unwrap().len(), 9);
    }
}
