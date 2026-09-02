// Reboot with a reason string, so Qualcomm's msm-restart driver stashes the
// matching magic in IMEM and aboot comes up in that mode.
//
// busybox's reboot cannot do this: it calls reboot(RESTART) with no argument,
// while selecting fastboot needs reboot(RESTART2, "bootloader"). Needs
// CAP_SYS_BOOT, so run it through AT+SYSCMD rather than from an adb shell.
//
//   rebootmode bootloader   -> fastboot
//   rebootmode recovery     -> recovery
//   rebootmode edl          -> emergency download (9008), if aboot honours it
//
// Prints what it is about to do and syncs first, because an unflushed
// filesystem after a hard restart is how you truncate the file you just wrote.

use std::ffi::CString;

const MAGIC1: libc::c_long = 0xfee1_dead_u32 as i32 as libc::c_long;
const MAGIC2: libc::c_long = 672_274_793;
const CMD_RESTART2: libc::c_long = 0xA1B2_C3D4_u32 as i32 as libc::c_long;

fn main() {
    let reason = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: rebootmode <bootloader|recovery|edl|...>");
        std::process::exit(2);
    });

    let c_reason = match CString::new(reason.clone()) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("reason must not contain a NUL byte");
            std::process::exit(2);
        }
    };

    eprintln!("rebooting with reason {reason:?}");
    unsafe {
        libc::sync();
    }
    // Give the flush a moment to reach NAND before the restart lands.
    std::thread::sleep(std::time::Duration::from_millis(1500));

    let rc = unsafe {
        libc::syscall(
            libc::SYS_reboot,
            MAGIC1,
            MAGIC2,
            CMD_RESTART2,
            c_reason.as_ptr(),
        )
    };

    // Only reached if the kernel refused; a successful call does not return.
    eprintln!("reboot syscall returned {rc}, errno {}", std::io::Error::last_os_error());
    std::process::exit(1);
}
