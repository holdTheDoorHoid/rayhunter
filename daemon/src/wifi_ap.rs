//! Switching the device's own WiFi access point off from the buttons.
//!
//! A hotspot running as a passive sensor does not need to be broadcasting a
//! network. Leaving it on costs battery and announces the device to anyone
//! looking, which is the opposite of what somebody carrying one usually wants.
//! See EFForg/rayhunter#849.
//!
//! **The access point is also how you reach the web interface**, so this is one
//! button gesture away from locking somebody out of their own device. Two
//! things prevent that.
//!
//! The gesture is deliberately hard to do by accident: several presses in quick
//! succession, both the count and the window being configurable. A device
//! rattling around in a bag does not produce that.
//!
//! And **restarting always brings the access point back**. That is not a
//! convention this code maintains, it is how the device behaves: the firmware
//! starts the access point at boot. So the worst case is a power cycle, which
//! is available to anybody holding the device, with no cable and no menu.
//!
//! Measured on an Orbic RC400L rather than assumed: stopping the access point
//! daemon takes the network down at once and the firmware does not put it back,
//! re-running the daemon by hand does *not* work, and a restart restores it
//! about thirty five seconds after boot.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use log::{error, info, warn};

/// How the access point comes back after being switched off.
///
/// Both routes end in a restart, because that is the only thing that reliably
/// works. `Temporary` schedules it, `UntilRestart` waits for the person to do
/// it. Neither can leave a device permanently unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum WifiApOffMode {
    /// Comes back on its own after a set time.
    Temporary,
    /// Stays off until the device is restarted.
    UntilRestart,
}

impl Default for WifiApOffMode {
    fn default() -> Self {
        // The safer of the two: it comes back without anybody having to
        // remember it was turned off.
        Self::Temporary
    }
}

/// The smallest number of presses worth accepting.
///
/// Two is a double tap, which is already a gesture this device uses for
/// something else and which a pocket can produce. Below four, a burst of
/// contact bounce starts to look like a deliberate gesture.
pub const MIN_PRESSES: u8 = 4;

/// Refuses to grow without bound if a button is held down against something.
pub const MAX_PRESSES: u8 = 20;

/// Recognises several presses in quick succession.
///
/// Kept as a plain state machine over timestamps, with no reference to buttons
/// or devices, so the thing that decides whether somebody meant it can be
/// tested. The alternative is finding out on hardware, which for a gesture that
/// switches off the only way into the device is not a good place to find out.
#[derive(Debug)]
pub struct PressGesture {
    presses_required: usize,
    window: Duration,
    recent: VecDeque<Instant>,
}

impl PressGesture {
    pub fn new(presses_required: u8, window: Duration) -> Self {
        let presses_required = presses_required.clamp(MIN_PRESSES, MAX_PRESSES) as usize;
        Self {
            presses_required,
            window,
            recent: VecDeque::with_capacity(presses_required),
        }
    }

    /// Record a press. Returns true when this one completes the gesture.
    ///
    /// Completing it clears the history, so holding a button down does not fire
    /// again on every press after the first.
    pub fn press(&mut self, now: Instant) -> bool {
        // Drop anything that has fallen out of the window. Done on the way in
        // rather than on a timer, so the gesture needs no clock of its own.
        while let Some(oldest) = self.recent.front() {
            if now.duration_since(*oldest) > self.window {
                self.recent.pop_front();
            } else {
                break;
            }
        }

        self.recent.push_back(now);
        if self.recent.len() >= self.presses_required {
            self.recent.clear();
            return true;
        }
        false
    }
}

/// The access point daemon, if one is running.
///
/// Found by looking through `/proc` rather than by assuming a path, because the
/// daemon differs between devices and a hardcoded name would silently do
/// nothing on the ones it does not match.
pub fn find_access_point_daemon() -> Option<(u32, String)> {
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        let name = cmdline.split_whitespace().next().unwrap_or_default();
        // The access point daemon on every device Rayhunter supports that has
        // one. Matched on the program name so a path does not have to be
        // guessed.
        if name.rsplit('/').next() == Some("hostapd") {
            return Some((pid, cmdline.trim().to_string()));
        }
    }
    None
}

/// Whether this device is one where the access point can be switched off.
///
/// Devices whose WiFi is run by something other than the usual daemon are
/// reported as unsupported rather than being guessed at. Turning a network off
/// with no idea how to bring it back is precisely what must not happen here.
pub fn is_supported() -> bool {
    find_access_point_daemon().is_some()
}

/// Switch the access point off.
///
/// Stops the daemon rather than unloading the driver. Unloading saves more
/// power but the driver has to be reloaded with the arguments the firmware
/// originally used, and getting those wrong leaves a device with no network and
/// no way to be told otherwise.
pub fn stop_access_point() -> Result<(), String> {
    let Some((pid, cmdline)) = find_access_point_daemon() else {
        return Err("no access point daemon is running on this device".to_string());
    };
    info!("switching the WiFi access point off (stopping pid {pid}: {cmdline})");

    // SIGTERM first: the daemon tells its clients it is going, which is tidier
    // than yanking the network out from under them.
    let killed = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if killed != 0 {
        return Err(format!(
            "could not stop the access point daemon: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Whether the access point is currently running.
pub fn is_access_point_running() -> bool {
    find_access_point_daemon().is_some()
}

/// Bring the access point back.
///
/// By restarting the device, because that is the only thing that works. On an
/// Orbic, re-running the daemon with the arguments it was started with exits
/// straight away and leaves the network down; the firmware brings it up at boot
/// and that is the path that has been seen to work.
///
/// The caller is responsible for anything that should happen before the device
/// goes down.
pub fn restore_access_point_by_restart() -> Result<(), String> {
    warn!("restarting to bring the WiFi access point back");
    // Flush first. This is an immediate reset, and a recording half written to
    // flash is worse than a few more seconds without WiFi.
    unsafe {
        libc::sync();
    }
    match std::fs::write("/proc/sysrq-trigger", "b") {
        Ok(()) => Ok(()),
        Err(err) => {
            error!("could not restart: {err}");
            Err(format!("could not restart: {err}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, millis: u64) -> Instant {
        base + Duration::from_millis(millis)
    }

    /// The whole point of the count: a stray press, or two, must do nothing.
    #[test]
    fn a_few_presses_do_not_fire_it() {
        let base = Instant::now();
        let mut gesture = PressGesture::new(5, Duration::from_secs(4));
        for i in 0..4 {
            assert!(
                !gesture.press(at(base, i * 200)),
                "fired after {} presses",
                i + 1
            );
        }
    }

    #[test]
    fn enough_presses_close_together_fire_it() {
        let base = Instant::now();
        let mut gesture = PressGesture::new(5, Duration::from_secs(4));
        for i in 0..4 {
            assert!(!gesture.press(at(base, i * 200)));
        }
        assert!(gesture.press(at(base, 800)), "the fifth press should fire");
    }

    /// Presses spread over a long time are somebody using the device, not
    /// somebody making a gesture.
    #[test]
    fn presses_spread_out_never_fire_it() {
        let base = Instant::now();
        let mut gesture = PressGesture::new(5, Duration::from_secs(4));
        for i in 0..20 {
            // One press every five seconds, outside a four second window.
            assert!(
                !gesture.press(at(base, i * 5_000)),
                "fired on slow press {i}"
            );
        }
    }

    /// Holding a button against something must fire at most once, not once per
    /// press for as long as it is held.
    #[test]
    fn firing_clears_the_history() {
        let base = Instant::now();
        let mut gesture = PressGesture::new(4, Duration::from_secs(4));
        let mut fired = 0;
        for i in 0..8 {
            if gesture.press(at(base, i * 100)) {
                fired += 1;
            }
        }
        assert_eq!(fired, 2, "eight presses should be two gestures, not five");
    }

    /// A count somebody typed into a config file has to be brought back into
    /// the range where this is still a deliberate gesture. Two presses is a
    /// double tap, which this device already uses for something else and which
    /// a pocket can produce.
    #[test]
    fn an_absurd_count_is_clamped() {
        let base = Instant::now();
        let mut gesture = PressGesture::new(1, Duration::from_secs(4));
        assert!(!gesture.press(at(base, 0)), "one press must never fire");
        assert!(!gesture.press(at(base, 100)));
        assert!(!gesture.press(at(base, 200)));
        assert!(
            gesture.press(at(base, 300)),
            "clamped up to the minimum of {MIN_PRESSES}"
        );

        let mut huge = PressGesture::new(u8::MAX, Duration::from_secs(60));
        let mut fired = false;
        for i in 0..(MAX_PRESSES as u64 + 1) {
            fired |= huge.press(at(base, i * 10));
        }
        assert!(fired, "clamped down to at most {MAX_PRESSES}");
    }

    /// The window slides rather than resetting, so a burst that starts slowly
    /// and finishes quickly still counts.
    #[test]
    fn the_window_slides() {
        let base = Instant::now();
        let mut gesture = PressGesture::new(4, Duration::from_secs(2));
        // Two presses, a long gap, then four quick ones. The stale pair must
        // fall out rather than counting towards the burst.
        assert!(!gesture.press(at(base, 0)));
        assert!(!gesture.press(at(base, 100)));
        assert!(!gesture.press(at(base, 10_000)));
        assert!(!gesture.press(at(base, 10_100)));
        assert!(!gesture.press(at(base, 10_200)));
        assert!(gesture.press(at(base, 10_300)));
    }

    #[test]
    fn temporary_is_the_default_way_back() {
        assert_eq!(WifiApOffMode::default(), WifiApOffMode::Temporary);
    }
}
