//! A second gate in front of the terminal: holding the unit.
//!
//! The terminal runs commands as root, which is the difference between an
//! interface that reads data and one that can do anything at all. A paired
//! browser is not enough for that, and neither is the passphrase alone: a
//! phone that has been taken, or a passphrase that has been shouldered, is
//! exactly the case. So the passphrase starts a step-up, the unit shows a
//! four-digit code on its own screen, and typing that code proves the person
//! at the browser can also see the unit. Units without a screen accept a
//! button press instead, which proves the same thing.
//!
//! A confirmed step-up opens a window of five minutes, extended by every
//! command and capped at thirty, per browser. While any window is open the
//! screen says so, so the person holding the unit can tell.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use log::{info, warn};

use crate::display::SharedOverride;
use crate::display::qr::{self, ScreenGeometry};
use crate::tls::random_bytes;

/// How long the code on the screen is good for.
pub const CODE_TTL: Duration = Duration::from_secs(60);
/// How long a confirmed step-up lasts without a command.
pub const WINDOW: Duration = Duration::from_secs(5 * 60);
/// The most a window can be stretched by using it.
pub const HARD_CAP: Duration = Duration::from_secs(30 * 60);
/// Wrong codes that end the attempt.
pub const MAX_WRONG: u8 = 3;
pub const BANNER: &str = "TERMINAL ACTIVE";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StepUpError {
    #[error("no step-up is waiting for a code; start one first")]
    NoPending,
    #[error("that code has expired; start again")]
    Expired,
    #[error("wrong code; {attempts_left} attempts left")]
    WrongCode { attempts_left: u8 },
    #[error("too many wrong codes; start again")]
    TooManyWrong,
    #[allow(dead_code)]
    #[error("this browser did not start the step-up")]
    NotYours,
}

struct Pending {
    device_id: String,
    code: String,
    started: Instant,
    wrong: u8,
}

struct Window {
    opened: Instant,
    last_used: Instant,
}

impl Window {
    fn expires_at(&self) -> Instant {
        (self.last_used + WINDOW).min(self.opened + HARD_CAP)
    }
}

/// The unit's screen, for the code and the banner.
#[derive(Clone)]
pub struct StepUpDisplay {
    pub override_: SharedOverride,
    pub screen: ScreenGeometry,
}

pub struct StepUp {
    /// One waiting code per browser, so one person starting a step-up does
    /// not throw away another's. A button press confirms the newest.
    pending: Mutex<HashMap<String, Pending>>,
    windows: Mutex<HashMap<String, Window>>,
    display: Option<StepUpDisplay>,
}

fn new_code() -> String {
    let mut raw = [0u8; 4];
    if random_bytes(&mut raw).is_err() {
        warn!("no randomness for a step-up code; refusing");
        return String::new();
    }
    format!("{:04}", u32::from_le_bytes(raw) % 10_000)
}

impl StepUp {
    pub fn new(display: Option<StepUpDisplay>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            windows: Mutex::new(HashMap::new()),
            display,
        }
    }

    pub fn has_screen(&self) -> bool {
        self.display.is_some()
    }

    /// Put a fresh code on the screen for `device_id`. The caller has
    /// already checked the passphrase.
    pub fn start(&self, device_id: &str) -> Result<Duration, StepUpError> {
        let code = new_code();
        if code.is_empty() {
            return Err(StepUpError::Expired);
        }
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.retain(|_, p| p.started.elapsed() < CODE_TTL);
            pending.insert(
                device_id.to_string(),
                Pending {
                    device_id: device_id.to_string(),
                    code: code.clone(),
                    started: Instant::now(),
                    wrong: 0,
                },
            );
        }
        match &self.display {
            Some(d) => {
                let px = qr::text_screen(&[("TERMINAL", 2), ("CODE", 2), (&code, 4)], d.screen);
                d.override_.show(px, CODE_TTL);
            }
            None => info!("step-up started with no screen; a button press confirms it"),
        }
        Ok(CODE_TTL)
    }

    /// Apply `check` to the pending request for `device_id` (or, with
    /// `None`, the newest one) and consume it on success or after too many
    /// wrong codes.
    fn take_pending_if(
        &self,
        device_id: Option<&str>,
        check: impl FnOnce(&mut Pending) -> Result<(), StepUpError>,
    ) -> Result<String, StepUpError> {
        let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|_, p| p.started.elapsed() < CODE_TTL);
        let key = match device_id {
            Some(id) => id.to_string(),
            None => guard
                .values()
                .max_by_key(|p| p.started)
                .map(|p| p.device_id.clone())
                .ok_or(StepUpError::NoPending)?,
        };
        let Some(p) = guard.get_mut(&key) else {
            return Err(StepUpError::NoPending);
        };
        match check(p) {
            Ok(()) => {
                guard.remove(&key);
                Ok(key)
            }
            Err(StepUpError::TooManyWrong) => {
                guard.remove(&key);
                Err(StepUpError::TooManyWrong)
            }
            Err(e) => Err(e),
        }
    }

    fn open_window(&self, device_id: String) -> Duration {
        let now = Instant::now();
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        windows.insert(
            device_id.clone(),
            Window {
                opened: now,
                last_used: now,
            },
        );
        drop(windows);
        if let Some(d) = &self.display {
            d.override_.clear();
        }
        self.refresh_banner();
        info!("terminal step-up confirmed for {device_id}");
        WINDOW
    }

    /// The code typed at the browser. Three wrong ones end the attempt.
    pub fn confirm(&self, device_id: &str, code: &str) -> Result<Duration, StepUpError> {
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        let owner = self.take_pending_if(Some(device_id), |p| {
            if crate::pairing::ct_eq(p.code.as_bytes(), code.as_bytes()) {
                return Ok(());
            }
            p.wrong += 1;
            if p.wrong >= MAX_WRONG {
                Err(StepUpError::TooManyWrong)
            } else {
                Err(StepUpError::WrongCode {
                    attempts_left: MAX_WRONG - p.wrong,
                })
            }
        })?;
        Ok(self.open_window(owner))
    }

    /// A button press on the unit confirms whatever step-up is waiting.
    /// This is the whole path on a unit with no screen, and works on the
    /// others too.
    pub fn button_pressed(&self) -> bool {
        match self.take_pending_if(None, |_| Ok(())) {
            Ok(owner) => {
                self.open_window(owner);
                true
            }
            Err(_) => false,
        }
    }

    /// Whether `device_id` has a live window, dropping it if it has lapsed.
    pub fn active(&self, device_id: &str) -> bool {
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let live = windows
            .get(device_id)
            .map(|w| now < w.expires_at())
            .unwrap_or(false);
        if !live && windows.remove(device_id).is_some() {
            drop(windows);
            self.refresh_banner();
        }
        live
    }

    /// Each command keeps the window open a little longer, up to the cap.
    pub fn extend(&self, device_id: &str) {
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = windows.get_mut(device_id) {
            w.last_used = Instant::now();
        }
        drop(windows);
        self.refresh_banner();
    }

    pub fn remaining(&self, device_id: &str) -> Option<Duration> {
        let windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        windows
            .get(device_id)
            .and_then(|w| w.expires_at().checked_duration_since(Instant::now()))
    }

    pub fn end(&self, device_id: &str) {
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        windows.remove(device_id);
        drop(windows);
        self.refresh_banner();
    }

    /// The banner stays up as long as the longest-lived window.
    fn refresh_banner(&self) {
        let Some(d) = &self.display else {
            return;
        };
        let now = Instant::now();
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        windows.retain(|_, w| now < w.expires_at());
        let longest = windows
            .values()
            .map(|w| w.expires_at())
            .max()
            .and_then(|at| at.checked_duration_since(now));
        drop(windows);
        match longest {
            Some(left) => d.override_.show_banner(BANNER, left),
            None => d.override_.clear_banner(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayOverride;
    use std::sync::Arc;

    fn with_screen() -> (StepUp, SharedOverride) {
        let o: SharedOverride = Arc::new(DisplayOverride::new());
        let s = StepUp::new(Some(StepUpDisplay {
            override_: o.clone(),
            screen: ScreenGeometry::square(128),
        }));
        (s, o)
    }

    fn code_of(s: &StepUp) -> String {
        s.pending
            .lock()
            .unwrap()
            .values()
            .max_by_key(|p| p.started)
            .map(|p| p.code.clone())
            .unwrap()
    }

    #[test]
    fn the_code_goes_on_the_screen_and_the_right_one_opens_a_window() {
        let (s, o) = with_screen();
        assert!(!s.active("phone"));
        s.start("phone").unwrap();
        assert!(o.active(), "the code is showing");
        let code = code_of(&s);
        assert_eq!(code.len(), 4);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));

        assert_eq!(
            s.confirm("laptop", &code),
            Err(StepUpError::NoPending),
            "another browser has nothing waiting and cannot claim it"
        );
        // Another browser starting its own does not disturb the first.
        s.start("laptop").unwrap();
        assert_eq!(s.pending.lock().unwrap().len(), 2);
        assert!(matches!(
            s.confirm("phone", "wrong"),
            Err(StepUpError::WrongCode { attempts_left: 2 })
        ));
        assert_eq!(s.confirm("phone", &code), Ok(WINDOW));
        assert!(s.active("phone"));
        assert!(!s.active("laptop"));
        assert!(!o.active(), "the code came down");
        assert_eq!(o.current_banner().as_deref(), Some(BANNER));
        // Once used, the code is gone.
        assert_eq!(s.confirm("phone", &code), Err(StepUpError::NoPending));
        s.end("phone");
        assert!(!s.active("phone"));
        assert!(
            o.current_banner().is_none(),
            "banner down with the last window"
        );
    }

    #[test]
    fn three_wrong_codes_end_the_attempt() {
        let (s, _) = with_screen();
        s.start("phone").unwrap();
        assert!(s.confirm("phone", "0000").is_err() || true);
        // Reset to a known state: start again and burn it.
        s.start("phone").unwrap();
        let code = code_of(&s);
        let wrong = if code == "1234" { "4321" } else { "1234" };
        assert!(matches!(
            s.confirm("phone", wrong),
            Err(StepUpError::WrongCode { .. })
        ));
        assert!(matches!(
            s.confirm("phone", wrong),
            Err(StepUpError::WrongCode { .. })
        ));
        assert_eq!(s.confirm("phone", wrong), Err(StepUpError::TooManyWrong));
        assert_eq!(s.confirm("phone", &code), Err(StepUpError::NoPending));
    }

    #[test]
    fn a_button_press_confirms_without_a_code() {
        let s = StepUp::new(None);
        assert!(!s.button_pressed(), "nothing waiting");
        s.start("phone").unwrap();
        assert!(s.button_pressed());
        assert!(s.active("phone"));
        assert!(s.remaining("phone").unwrap() <= WINDOW);
    }

    #[test]
    fn using_the_terminal_extends_the_window_but_not_past_the_cap() {
        let now = Instant::now();
        let w = Window {
            opened: now,
            last_used: now,
        };
        assert_eq!(w.expires_at(), now + WINDOW);
        let late = Window {
            opened: now,
            last_used: now + HARD_CAP,
        };
        assert_eq!(late.expires_at(), now + HARD_CAP);
    }
}
