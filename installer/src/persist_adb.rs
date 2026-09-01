//! Making ADB survive a reboot.
//!
//! The installer can already turn ADB on, but only until the device restarts:
//! the boot scripts reset the USB composition every time. People who need ADB
//! for anything ongoing have been copying shell scripts out of GitHub
//! discussions to work around it, which is a support burden and an easy thing
//! to get wrong on a device where getting it wrong means a reflash.
//!
//! Folded in here from EFForg/rayhunter#893 (Moxee) and #928 (Wingtech and
//! T-Mobile), keeping the device-side commands identical to the ones people
//! have actually run.
//!
//! **This changes how the device boots.** Both mechanisms are reversible, and
//! `--revert` puts each back, but that is the reason the commands are printed
//! as they run rather than done quietly.

use anyhow::{Context, Result, bail};
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

/// Selects the USB composition the Moxee boots into.
///
/// `/usrdata` is a persistent read-write partition, which is what makes
/// writing here stick where the composition command does not.
pub const MOXEE_MODE_FILE: &str = "/usrdata/mode.cfg";

/// RNDIS, DIAG, serial and ADB. Product id becomes `0xF622`.
pub const MOXEE_MODE_WITH_ADB: &str = "9";

/// RNDIS only, the factory setting. Product id `0xF626`.
pub const MOXEE_MODE_DEFAULT: &str = "3";

/// The boot script added on Wingtech and T-Mobile hardware.
pub const WINGTECH_INIT_PATH: &str = "/etc/init.d/enable_adb";

/// Run after `S30usb`, which forces a composition without ADB on every boot.
/// The point of the number is to land immediately after it.
pub const WINGTECH_RC_LINK: &str = "/etc/rcS.d/S31enable_adb";

/// The composition with DIAG, ADB, modem, NMEA and QMI.
pub const WINGTECH_ADB_COMPOSITION: &str = "9025";

/// The boot script, as the body of a `printf`.
///
/// Written with `printf` inside single quotes so that the shell this reaches
/// does not expand `$1` on the way. Deliberately free of `&`, `+` and `%`:
/// this travels through a form encoded request body on the way to the device,
/// where each of those means something else. That constraint is why the script
/// is this plain, and `wingtech_command_is_safe_to_send` is what keeps it so.
pub fn wingtech_script() -> String {
    format!(
        "#!/bin/sh\\ncase $1 in\\nstart) /sbin/usb/compositions/{WINGTECH_ADB_COMPOSITION} n ;;\\nesac\\n"
    )
}

/// The commands that install the boot script, in order.
///
/// Split out and pure so the sequence can be read and tested without a device.
/// The root filesystem is mounted read-only, hence the remounts either side;
/// the read-only remount at the end matters, because leaving a device's root
/// writable is not a state to walk away from.
pub fn wingtech_install_commands() -> Vec<String> {
    vec![
        "mount -o remount,rw /".to_string(),
        format!("printf '{}' > {WINGTECH_INIT_PATH}", wingtech_script()),
        format!("chmod 755 {WINGTECH_INIT_PATH}"),
        format!("ln -sf ../init.d/enable_adb {WINGTECH_RC_LINK}"),
        "mount -o remount,ro /".to_string(),
    ]
}

/// The commands that take it back out again.
pub fn wingtech_revert_commands() -> Vec<String> {
    vec![
        "mount -o remount,rw /".to_string(),
        format!("rm -f {WINGTECH_RC_LINK}"),
        format!("rm -f {WINGTECH_INIT_PATH}"),
        "mount -o remount,ro /".to_string(),
    ]
}

/// Whether a command can survive the trip to a Wingtech.
///
/// It is delivered inside a form encoded body, where `&` starts the next
/// field, `+` decodes to a space and `%` starts an escape. Any of those would
/// arrive as something other than what was written, and on a command that
/// remounts a root filesystem and writes a boot script, arriving as something
/// else is the worst possible outcome. Checked rather than assumed.
pub fn wingtech_command_is_safe_to_send(command: &str) -> bool {
    !command.contains(['&', '+', '%', '\n', '\r'])
}

/// Make ADB persist on a Moxee.
///
/// Reads the value back afterwards, which is possible here because this device
/// is reached over a telnet shell rather than through a one-way request.
pub async fn moxee(
    admin_ip: &str,
    admin_username: &str,
    admin_password: Option<&str>,
    revert: bool,
) -> Result<()> {
    let wanted = if revert {
        MOXEE_MODE_DEFAULT
    } else {
        MOXEE_MODE_WITH_ADB
    };

    println!("Logging in and starting telnet...");
    crate::orbic_network::start_telnet(admin_ip, admin_username, admin_password).await?;

    let addr = SocketAddr::from_str(&format!("{admin_ip}:23"))
        .with_context(|| format!("{admin_ip} is not an address"))?;
    let timeout = Duration::from_secs(20);

    let command = format!("echo {wanted} > {MOXEE_MODE_FILE}");
    println!("Running: {command}");
    crate::util::telnet_send_command(addr, &command, "command done, exit code 0", true).await?;

    // Read it back. Writing the file and hoping is not worth much when the
    // consequence of it not having worked is somebody power cycling a device
    // and wondering why nothing changed.
    let readback = crate::util::telnet_send_command_with_output(
        addr,
        &format!("cat {MOXEE_MODE_FILE}"),
        true,
        timeout,
    )
    .await?;
    if !readback.contains(wanted) {
        bail!("wrote {MOXEE_MODE_FILE} but read back {readback:?}, expected {wanted}");
    }

    if revert {
        println!("Done. {MOXEE_MODE_FILE} is back to {MOXEE_MODE_DEFAULT} (RNDIS only).");
    } else {
        println!("Done. {MOXEE_MODE_FILE} is {MOXEE_MODE_WITH_ADB} (RNDIS, DIAG, serial, ADB).");
    }
    println!("Power cycle the device for this to take effect.");
    println!(
        "Note: a device booted on USB power alone disables USB regardless of this setting, so power it off USB first."
    );
    Ok(())
}

/// Make ADB persist on a Wingtech CT2MHS01 or T-Mobile TMOHS1.
///
/// Unlike the Moxee, these are driven through the admin interface, which
/// returns nothing useful, so there is no way to read the result back. Each
/// command is printed as it is sent so that a failure can be traced, and the
/// device has to be checked by hand afterwards.
pub async fn wingtech(
    admin_ip: &str,
    admin_password: &str,
    revert: bool,
    device_label: &str,
) -> Result<()> {
    let commands = if revert {
        wingtech_revert_commands()
    } else {
        wingtech_install_commands()
    };

    for command in &commands {
        if !wingtech_command_is_safe_to_send(command) {
            bail!("refusing to send a command that would not survive the trip: {command:?}");
        }
    }

    if revert {
        println!("Removing the persistent ADB boot script from the {device_label}...");
    } else {
        println!("Installing a persistent ADB boot script on the {device_label}...");
    }

    for command in &commands {
        println!("  {command}");
        crate::wingtech::run_command(admin_ip, admin_password, command)
            .await
            .with_context(|| format!("failed running: {command}"))?;
    }

    println!();
    if revert {
        println!("Done. ADB will no longer be enabled at boot.");
    } else {
        println!("Done. ADB should now be enabled on every boot.");
        println!(
            "This cannot be confirmed from here: the admin interface returns nothing to read back."
        );
        println!("Reboot the device and check `adb devices`.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands travel inside a form encoded body. A `&` would truncate
    /// one, and a truncated `mount -o remount,rw /` is a device left with a
    /// writable root and no boot script.
    #[test]
    fn every_command_survives_the_trip() {
        for command in wingtech_install_commands()
            .iter()
            .chain(wingtech_revert_commands().iter())
        {
            assert!(
                wingtech_command_is_safe_to_send(command),
                "unsafe to send: {command}"
            );
        }
    }

    #[test]
    fn the_safety_check_actually_rejects_things() {
        assert!(!wingtech_command_is_safe_to_send("a & b"));
        assert!(!wingtech_command_is_safe_to_send("a + b"));
        assert!(!wingtech_command_is_safe_to_send("100%"));
        assert!(!wingtech_command_is_safe_to_send("two\nlines"));
        assert!(wingtech_command_is_safe_to_send("mount -o remount,rw /"));
    }

    /// The script has to be quoted so `$1` reaches the file rather than being
    /// expanded by the shell that writes it. An unquoted one silently writes a
    /// script that always runs, whatever argument the boot gives it.
    #[test]
    fn the_script_is_written_inside_single_quotes() {
        let write = &wingtech_install_commands()[1];
        assert!(write.contains("printf '"), "{write}");
        assert!(write.contains("$1"), "{write}");
        assert!(write.ends_with(WINGTECH_INIT_PATH), "{write}");
    }

    /// The order matters: remount writable, write, link, remount read-only.
    /// Leaving a device's root filesystem writable is not a state to walk away
    /// from.
    #[test]
    fn the_root_filesystem_is_left_read_only() {
        for commands in [wingtech_install_commands(), wingtech_revert_commands()] {
            assert_eq!(commands.first().unwrap(), "mount -o remount,rw /");
            assert_eq!(commands.last().unwrap(), "mount -o remount,ro /");
        }
    }

    #[test]
    fn the_boot_script_runs_the_composition_with_adb() {
        let script = wingtech_script();
        assert!(script.contains(WINGTECH_ADB_COMPOSITION), "{script}");
        // Guarded on start, so a stop does not turn ADB back on.
        assert!(script.contains("case $1 in"), "{script}");
        assert!(script.contains("start)"), "{script}");
    }

    /// The link has to sort after S30usb, which is what resets the composition
    /// on every boot. Landing before it would be silently useless.
    #[test]
    fn the_boot_link_runs_after_the_usb_script() {
        assert!(WINGTECH_RC_LINK.contains("S31"), "{WINGTECH_RC_LINK}");
    }

    #[test]
    fn revert_removes_both_the_link_and_the_script() {
        let commands = wingtech_revert_commands().join("\n");
        assert!(commands.contains(WINGTECH_RC_LINK));
        assert!(commands.contains(WINGTECH_INIT_PATH));
    }

    /// Moxee's two values are what the composition script reads; swapping them
    /// would turn ADB off while claiming to turn it on.
    #[test]
    fn moxee_modes_are_the_documented_values() {
        assert_eq!(MOXEE_MODE_WITH_ADB, "9");
        assert_eq!(MOXEE_MODE_DEFAULT, "3");
        assert_eq!(MOXEE_MODE_FILE, "/usrdata/mode.cfg");
    }
}
