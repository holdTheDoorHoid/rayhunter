use log::info;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config;
use crate::display::{DisplayState, SharedOverride, SharedSuppression};

pub fn update_ui(
    task_tracker: &TaskTracker,
    _config: &config::Config,
    // This display never covers the device's own screens, so there is
    // nothing for a button press to step aside for. Taken anyway to keep
    // one signature across every device.
    _suppression: SharedSuppression,
    // No screen to put a pairing code on.
    _override: SharedOverride,
    shutdown_token: CancellationToken,
    mut ui_update_rx: Receiver<DisplayState>,
) {
    info!("Headless mode, not spawning UI.");
    task_tracker.spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = ui_update_rx.recv() => {}
            }
        }
    });
}
