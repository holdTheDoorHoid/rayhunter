use crate::config;
use crate::config::KeepScreenOn;
use crate::display::DisplayState;
use crate::display::generic_framebuffer::{self, Dimensions, GenericFramebuffer};
use async_trait::async_trait;
use log::{debug, info, warn};
use std::time::Duration;

use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

const FB_PATH: &str = "/dev/fb0";

/// Display controls on the Orbic's panel, found under the SPI device.
///
/// `sleep_mode` and `bl_gpio` both read 0 once the stock firmware has blanked
/// the screen on its own timer. Writing 1 back to them brings it up again.
/// Writing the framebuffer does not count as activity to that timer, which is
/// the whole reason the screen goes dark while Rayhunter is plainly running.
const PANEL_DIR: &str = "/sys/devices/78b6000.spi/spi_master/spi1/spi1.0";
const PANEL_SLEEP_MODE: &str = "/sys/devices/78b6000.spi/spi_master/spi1/spi1.0/sleep_mode";
const PANEL_BACKLIGHT: &str = "/sys/devices/78b6000.spi/spi_master/spi1/spi1.0/bl_gpio";

/// System-wide auto-suspend. "mem" suspends, "off" does not.
///
/// Left at "mem" the device suspends and resumes underneath us, which makes
/// the panel flicker even while it is being woken.
const AUTOSLEEP: &str = "/sys/power/autosleep";

/// Whether an external supply is connected.
///
/// Deliberately not the charger's own `chg_en`, which is what the battery
/// module reads. That flag means "currently charging", so it reads 0 on a
/// device sitting on USB with a full battery, which is precisely the desk
/// setup this feature is for. `online` in the kernel's power supply class
/// means an external supply is present, which is the actual question.
const USB_ONLINE: &str = "/sys/class/power_supply/usb/online";

/// How often to look at the panel state.
///
/// The stock firmware blanks the screen on its own schedule and does not tell
/// us, so this has to be a poll. Each pass is two small sysfs reads, and half
/// a second is short enough that a blank is never really seen.
const POLL: Duration = Duration::from_millis(500);

async fn read_flag(path: &str) -> Option<bool> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => match s.trim() {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        },
        Err(_) => None,
    }
}

async fn write_sysfs(path: &str, value: &str) {
    if let Err(e) = tokio::fs::write(path, value).await {
        warn!("keep_screen_on: failed writing '{value}' to {path}: {e}");
    }
}

/// Hold the panel awake for as long as the setting says to.
///
/// Improves on the upstream attempt (EFForg/rayhunter#919, closed) in three
/// ways. It can be limited to while external power is connected, which was the
/// maintainer's main objection to merging it. It restores auto-suspend as soon
/// as it stops holding the screen rather than only at shutdown, so unplugging
/// a device set to `WhenPluggedIn` gives its battery life straight back. And
/// it shuts down promptly instead of after up to a full poll interval.
fn spawn_keep_screen_on(
    task_tracker: &TaskTracker,
    mode: KeepScreenOn,
    shutdown_token: CancellationToken,
) {
    task_tracker.spawn(async move {
        if tokio::fs::metadata(PANEL_DIR).await.is_err() {
            warn!("keep_screen_on is set, but this device has no {PANEL_DIR}; doing nothing");
            return;
        }
        let can_autosleep = tokio::fs::metadata(AUTOSLEEP).await.is_ok();
        if !can_autosleep {
            warn!("keep_screen_on: no {AUTOSLEEP}; the screen may still flicker as the device suspends");
        }
        info!("keep_screen_on: watching the panel ({mode:?})");

        // Tracks whether we are the reason auto-suspend is off, so that it is
        // put back exactly once when we stop holding the screen.
        let mut suspended_autosleep = false;

        loop {
            let holding = match mode {
                KeepScreenOn::Never => false,
                KeepScreenOn::Always => true,
                // An unreadable flag is treated as unplugged. Failing towards
                // letting the screen sleep spends nobody's battery.
                KeepScreenOn::WhenPluggedIn => read_flag(USB_ONLINE).await.unwrap_or(false),
            };

            if holding {
                if can_autosleep && !suspended_autosleep {
                    write_sysfs(AUTOSLEEP, "off").await;
                    suspended_autosleep = true;
                }
                // Either flag at 0 means the panel has been blanked.
                let asleep = read_flag(PANEL_SLEEP_MODE).await == Some(false)
                    || read_flag(PANEL_BACKLIGHT).await == Some(false);
                if asleep {
                    debug!("keep_screen_on: panel blanked, waking it");
                    // Backlight first, then the panel itself: the other order
                    // shows a lit blank screen for an instant.
                    write_sysfs(PANEL_BACKLIGHT, "1").await;
                    write_sysfs(PANEL_SLEEP_MODE, "1").await;
                }
            } else if suspended_autosleep {
                // Stopped holding, so give the power saving back right away
                // rather than at shutdown. This is what makes WhenPluggedIn
                // worth having: unplug the device and it saves power again.
                write_sysfs(AUTOSLEEP, "mem").await;
                suspended_autosleep = false;
            }

            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = tokio::time::sleep(POLL) => {}
            }
        }

        if suspended_autosleep {
            debug!("keep_screen_on: restoring auto-suspend on the way out");
            write_sysfs(AUTOSLEEP, "mem").await;
        }
    });
}

#[derive(Copy, Clone, Default)]
struct Framebuffer;

#[async_trait]
impl GenericFramebuffer for Framebuffer {
    fn dimensions(&self) -> Dimensions {
        // TODO actually poll for this, maybe w/ fbset?
        Dimensions {
            height: 128,
            width: 128,
        }
    }

    async fn write_buffer(&mut self, buffer: Vec<(u8, u8, u8)>) {
        let mut raw_buffer = Vec::with_capacity(buffer.len() * 2);
        for (r, g, b) in buffer {
            let mut rgb565: u16 = (r as u16 & 0b11111000) << 8;
            rgb565 |= (g as u16 & 0b11111100) << 3;
            rgb565 |= (b as u16) >> 3;
            raw_buffer.extend(rgb565.to_le_bytes());
        }

        tokio::fs::write(FB_PATH, &raw_buffer).await.unwrap();
    }
}

pub fn update_ui(
    task_tracker: &TaskTracker,
    config: &config::Config,
    shutdown_token: CancellationToken,
    ui_update_rx: Receiver<DisplayState>,
) {
    if config.keep_screen_on != KeepScreenOn::Never {
        spawn_keep_screen_on(task_tracker, config.keep_screen_on, shutdown_token.clone());
    }

    generic_framebuffer::update_ui(
        task_tracker,
        config,
        Framebuffer,
        shutdown_token,
        ui_update_rx,
    )
}
