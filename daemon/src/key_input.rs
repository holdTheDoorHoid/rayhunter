use log::{error, info, warn};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::{self, KeyInputMode};
use crate::diag::DiagDeviceCtrlMessage;
use crate::display::{KEYPRESS_QUIET_PERIOD, SharedSuppression};

#[derive(Debug)]
enum Event {
    KeyDown,
    KeyUp,
}

const INPUT_EVENT_SIZE: usize = 32;

/// The power button on every supported device.
const INPUT_DEVICE: &str = "/dev/input/event0";

pub fn run_key_input_thread(
    task_tracker: &TaskTracker,
    config: &config::Config,
    diag_tx: Sender<DiagDeviceCtrlMessage>,
    suppression: SharedSuppression,
    // A button press on a unit nobody has paired with yet opens the setup
    // window again. Somebody pressing buttons on an unpaired unit is
    // somebody holding it, which is the whole basis for trusting the code
    // on its screen.
    pairing: Option<std::sync::Arc<crate::pairing::Pairing>>,
    // A press also confirms a waiting terminal step-up, on any unit.
    stepup: Option<std::sync::Arc<crate::stepup::StepUp>>,
    cancellation_token: CancellationToken,
) {
    let restart_on_double_tap = config.key_input_mode != KeyInputMode::Disabled;
    let pause_on_keypress = config.pause_display_on_keypress;
    // Switching the access point off from the buttons, when it is switched on
    // and this device is one where it can be done at all. Checked here so a
    // device with a WiFi stack we cannot control never pretends to offer it.
    let ap_toggle = config.wifi_ap_button_toggle && crate::wifi_ap::is_supported();
    if config.wifi_ap_button_toggle && !ap_toggle {
        warn!(
            "the WiFi access point button gesture is switched on, but this device has no access point daemon we know how to stop; ignoring it"
        );
    }
    let mut ap_gesture = crate::wifi_ap::PressGesture::new(
        config.wifi_ap_toggle_presses,
        Duration::from_secs(config.wifi_ap_toggle_window_secs.max(1)),
    );
    let ap_off_mode = config.wifi_ap_off_mode;
    let ap_off_minutes = config.wifi_ap_off_minutes.max(1);

    // Reading the buttons is now worth doing for either reason, so the thread
    // runs if either wants it. Previously it started only for the double tap
    // gesture, which would have left the pause feature dead for anyone who had
    // button control switched off.
    if !restart_on_double_tap && !pause_on_keypress && !ap_toggle && pairing.is_none() {
        return;
    }

    task_tracker.spawn(async move {
        let file = match open_input(INPUT_DEVICE) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to open {INPUT_DEVICE}: {e}");
                return;
            }
        };

        let mut buffer = [0u8; INPUT_EVENT_SIZE];
        let mut last_keyup: Option<Instant> = None;
        let mut last_event_time: Option<Instant> = None;

        loop {
            tokio::select! {
               _ = cancellation_token.cancelled() => {
                    info!("received key input shutdown");
                    return;
                }
                result = read_full(&file, &mut buffer) => {
                    if let Err(e) = result {
                        error!("failed to read key input: {e}");
                        return;
                    }
                }
            }

            let event = parse_event(buffer);

            let now = Instant::now();

            // Any button activity at all, before the debounce below. Somebody
            // navigating the device's menus is exactly the repeated, bouncy
            // input that debounce throws away, and they are the person this is
            // for.
            if pause_on_keypress {
                suppression.suppress_for(KEYPRESS_QUIET_PERIOD);
            }

            // On orbic it was observed that pressing the power button can trigger many successive
            // events. Drop events that are too close together.
            if let Some(last_time) = last_event_time
                && now.duration_since(last_time) < Duration::from_millis(50)
            {
                last_event_time = Some(now);
                continue;
            }
            last_event_time = Some(now);

            match event {
                Event::KeyUp => {
                    // Refused, cheaply, once the unit has an owner; until
                    // then every press re-arms the window. In the
                    // background so a slow flash write never delays the
                    // gestures below.
                    if let Some(stepup) = &stepup
                        && stepup.button_pressed()
                    {
                        info!("button press confirmed a terminal step-up");
                    }
                    if let Some(pairing) = &pairing {
                        let pairing = pairing.clone();
                        tokio::spawn(async move { pairing.button_pressed().await });
                    }

                    // Checked before the double tap below, and on its own
                    // timer, so the two gestures do not have to agree about
                    // what counts as a press.
                    if ap_toggle && ap_gesture.press(now) {
                        toggle_access_point(ap_off_mode, ap_off_minutes);
                        last_keyup = None;
                        continue;
                    }

                    if let Some(last_keyup_instant) = last_keyup {
                        let elapsed = now.duration_since(last_keyup_instant);

                        if restart_on_double_tap
                            && elapsed >= Duration::from_millis(100)
                            && elapsed <= Duration::from_millis(800)
                        {
                            if let Err(e) = diag_tx.send(DiagDeviceCtrlMessage::StopRecording).await
                            {
                                error!("Failed to send StopRecording: {e}");
                            }
                            if let Err(e) = diag_tx
                                .send(DiagDeviceCtrlMessage::StartRecording { response_tx: None })
                                .await
                            {
                                error!("Failed to send StartRecording: {e}");
                            }
                            last_keyup = None;
                            continue;
                        }
                    }

                    last_keyup = Some(now);
                }
                Event::KeyDown => {}
            }
        }
    });
}

/// Act on the gesture: switch the access point off, or restart to bring it
/// back.
///
/// Both directions are deliberately blunt. Turning it off stops the daemon,
/// which takes the network down at once. Turning it back on restarts the
/// device, because on the hardware this was measured on, re-running the daemon
/// by hand does not work and the firmware only brings it up at boot.
fn toggle_access_point(mode: crate::wifi_ap::WifiApOffMode, off_minutes: u64) {
    use crate::wifi_ap;

    if wifi_ap::is_access_point_running() {
        match wifi_ap::stop_access_point() {
            Ok(()) => {
                info!("WiFi access point switched off by button gesture");
                if mode == wifi_ap::WifiApOffMode::Temporary {
                    // Restarting is how it comes back, so the timer restarts
                    // the device. Spawned rather than awaited so the buttons
                    // keep working in the meantime, including to bring the
                    // access point back sooner.
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(off_minutes * 60)).await;
                        info!("bringing the WiFi access point back after {off_minutes} minutes");
                        if let Err(err) = wifi_ap::restore_access_point_by_restart() {
                            error!("could not bring the access point back: {err}");
                        }
                    });
                }
            }
            Err(err) => error!("could not switch the access point off: {err}"),
        }
    } else {
        // Already off, so the gesture means "bring it back".
        if let Err(err) = wifi_ap::restore_access_point_by_restart() {
            error!("could not bring the access point back: {err}");
        }
    }
}

/// Open the button node for the runtime's I/O driver to wait on.
///
/// Not `tokio::fs::File`: that runs every read on the blocking pool, and a
/// read of an input node returns only when a button is pressed. On shutdown
/// the `select!` in the reader dropped the read future but not the `read(2)`
/// behind it, which stayed in `evdev_read` on a pool thread until the next
/// press. Dropping the runtime at the end of `main` waits for the blocking
/// pool, so the process never exited: `stop` returned, the log ended with
/// "see you space cowboy", and the pid stayed until `kill -9`. A
/// non-blocking descriptor registered with epoll costs no thread while it
/// waits, and a dropped wait is simply forgotten.
fn open_input(path: &str) -> std::io::Result<AsyncFd<std::fs::File>> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    AsyncFd::with_interest(file, Interest::READABLE)
}

/// Fill `buf` completely, as `read_exact` did, so a key event and the
/// SYN_REPORT the kernel sends after it are always consumed together.
async fn read_full<T>(fd: &AsyncFd<T>, buf: &mut [u8]) -> std::io::Result<()>
where
    T: AsRawFd,
    for<'a> &'a T: Read,
{
    let mut filled = 0;
    while filled < buf.len() {
        let mut guard = fd.readable().await?;
        match guard.try_io(|inner| Read::read(&mut inner.get_ref(), &mut buf[filled..])) {
            Ok(Ok(0)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "input device closed",
                ));
            }
            Ok(Ok(n)) => filled += n,
            Ok(Err(e)) => return Err(e),
            // Readiness was stale: wait for the next.
            Err(_would_block) => {}
        }
    }
    Ok(())
}

fn parse_event(input: [u8; INPUT_EVENT_SIZE]) -> Event {
    if input[12] == 0 {
        Event::KeyUp
    } else {
        Event::KeyDown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    /// Two connected sockets, the far end standing in for the kernel: what
    /// it writes is what a button press would deliver.
    fn input_pair() -> (AsyncFd<UnixStream>, UnixStream) {
        let (near, far) = UnixStream::pair().unwrap();
        near.set_nonblocking(true).unwrap();
        (
            AsyncFd::with_interest(near, Interest::READABLE).unwrap(),
            far,
        )
    }

    #[tokio::test]
    async fn read_full_gathers_a_whole_buffer_across_short_reads() {
        let (near, mut far) = input_pair();
        let mut buf = [0u8; INPUT_EVENT_SIZE];
        far.write_all(&[1u8; 16]).unwrap();
        // The second half arrives later, as a SYN_REPORT after its key event
        // could.
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            far.write_all(&[2u8; 16]).unwrap();
            far
        });
        read_full(&near, &mut buf).await.unwrap();
        assert_eq!(&buf[..16], &[1u8; 16]);
        assert_eq!(&buf[16..], &[2u8; 16]);
        // A closed device is an error, not a spin.
        drop(writer.join().unwrap());
        let err = read_full(&near, &mut buf).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    /// The bug this guards against: a wait for a press that never comes must
    /// not outlive the reader. With `tokio::fs::File` the abandoned read sat
    /// on a blocking-pool thread, and dropping the runtime, which waits for
    /// that pool, hung the process at exit.
    #[test]
    fn an_abandoned_wait_does_not_hold_the_runtime() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let far = rt.block_on(async {
            let (near, far) = input_pair();
            let mut buf = [0u8; INPUT_EVENT_SIZE];
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                res = read_full(&near, &mut buf) => panic!("nothing was written, got {res:?}"),
            }
            far
        });
        let dropped = std::thread::spawn(move || drop(rt));
        let started = Instant::now();
        while !dropped.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "runtime drop is waiting on the abandoned read"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(far);
    }

    #[test]
    fn test_parse_event_keydown_m7350_v5() {
        let input = [
            0x57, 0x6c, 0x09, 0x00, 0x7c, 0xfb, 0x03, 0x00, 0x01, 0x00, 0x74, 0x00, 0x01, 0x00,
            0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert!(matches!(parse_event(input), Event::KeyDown));
    }

    #[test]
    fn test_parse_event_keyup_m7350_v5() {
        let input = [
            0x57, 0x6c, 0x09, 0x00, 0x1b, 0x15, 0x05, 0x00, 0x01, 0x00, 0x74, 0x00, 0x00, 0x00,
            0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert!(matches!(parse_event(input), Event::KeyUp));
    }
}
