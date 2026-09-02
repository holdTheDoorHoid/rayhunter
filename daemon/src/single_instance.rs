//! One daemon per device.
//!
//! The init script cannot be trusted to prevent a second start: its pidfile
//! check looks for `/bin/sh`, which the daemon stops being the moment it
//! `exec`s, and a stop that outruns a slow shutdown leaves the old process
//! behind. Two instances then fight over the diag device and the web ports,
//! and the one that answers the web interface is not the one recording. So
//! the daemon takes an exclusive lock on a file at startup and refuses to
//! run while another holds it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

/// Where the lock lives. `/tmp` is memory on every supported device, so a
/// stale file cannot survive a reboot.
pub const LOCK_PATH: &str = "/tmp/rayhunter-daemon.lock";

/// Held for the life of the process; dropping it releases the lock.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// Take the lock at `path`, or say who has it.
pub fn acquire(path: &Path) -> Result<InstanceLock, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    // SAFETY: flock on a valid, open descriptor; no memory is handed over.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        let mut holder = String::new();
        let _ = file.read_to_string(&mut holder);
        let holder = holder.trim();
        return Err(if holder.is_empty() {
            format!("another rayhunter-daemon is already running ({err})")
        } else {
            format!("another rayhunter-daemon is already running (pid {holder})")
        });
    }
    // Record who holds it, for the message the next one prints.
    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = writeln!(file, "{}", std::process::id());
    let _ = file.flush();
    Ok(InstanceLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_holder_is_refused_until_the_first_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let first = acquire(&path).expect("first lock");
        let refused = acquire(&path).expect_err("second lock must fail");
        assert!(
            refused.contains(&format!("pid {}", std::process::id())),
            "{refused}"
        );
        drop(first);
        acquire(&path).expect("lock free again");
    }

    #[test]
    fn an_unwritable_place_is_reported() {
        let err = acquire(Path::new("/nonexistent/rayhunter/lock")).unwrap_err();
        assert!(err.contains("cannot open"), "{err}");
    }
}
