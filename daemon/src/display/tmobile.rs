/// Display module for Tmobile TMOHS1, blink LEDs on the front of the device.
/// DisplayState::Recording => Signal LED slowly blinks blue.
/// DisplayState::Paused => WiFi LED blinks white.
/// DisplayState::WarningDetected { .. } => Signal LED slowly blinks red.
use log::{error, info};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use std::time::Duration;

use crate::config::{self, UiLevel};
use crate::display::{DisplayState, SharedOverride, SharedSuppression};

macro_rules! led {
    ($l:expr) => {{ format!("/sys/class/leds/led:{}/blink", $l) }};
}

async fn start_blinking(path: String) {
    tokio::fs::write(&path, "1").await.ok();
}

async fn stop_blinking(path: String) {
    tokio::fs::write(&path, "0").await.ok();
}

pub fn update_ui(
    task_tracker: &TaskTracker,
    config: &config::Config,
    // This display never covers the device's own screens, so there is
    // nothing for a button press to step aside for. Taken anyway to keep
    // one signature across every device.
    _suppression: SharedSuppression,
    // No screen to put a pairing code on; these units pair by button press.
    _override: SharedOverride,
    shutdown_token: CancellationToken,
    mut ui_update_rx: mpsc::Receiver<DisplayState>,
) {
    let mut invisible: bool = false;
    if config.ui_level == UiLevel::Invisible {
        info!("Invisible mode, not spawning UI.");
        invisible = true;
    }
    task_tracker.spawn(async move {
        let mut state = DisplayState::Recording;
        let mut last_state = DisplayState::Paused;

        loop {
            if shutdown_token.is_cancelled() {
                info!("received UI shutdown");
                break;
            }
            match ui_update_rx.try_recv() {
                Ok(new_state) => state = new_state,
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(e) => error!("error receiving ui update message: {e}"),
            };
            if invisible || state == last_state {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            match state {
                DisplayState::Paused => {
                    stop_blinking(led!("signal_blue")).await;
                    stop_blinking(led!("signal_red")).await;
                    start_blinking(led!("wlan_white")).await;
                }
                DisplayState::Recording => {
                    stop_blinking(led!("wlan_white")).await;
                    stop_blinking(led!("signal_red")).await;
                    start_blinking(led!("signal_blue")).await;
                }
                DisplayState::WarningDetected { .. } => {
                    stop_blinking(led!("wlan_white")).await;
                    stop_blinking(led!("signal_blue")).await;
                    start_blinking(led!("signal_red")).await;
                }
            }
            last_state = state;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
