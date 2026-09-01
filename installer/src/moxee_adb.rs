//! Installing on a Moxee over ADB, with no admin password.
//!
//! The ordinary Moxee install goes through the admin web interface, which means
//! typing the device's admin password. That cannot be avoided the first time:
//! ADB is off, and the password is the only way in.
//!
//! Once ADB has been made to persist, though, it does not need to be typed
//! again. This path installs over ADB instead, which is simpler in every way
//! that matters: no login, no exploit, no telnet, and a transfer that can be
//! checked byte for byte.
//!
//! Unlike the Orbic, whose ADB runs as an unprivileged user and needs commands
//! routed through `rootshell`, **the Moxee's ADB is already root**. Measured on
//! a K779HSDL: `id` reports uid 0 and files under the data directory are
//! writable directly. So there is no privilege dance here.

use adb_client::{ADBDeviceExt, ADBUSBDevice};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::RAYHUNTER_DAEMON_INIT;
use crate::connection::{
    DeviceConnection, install_config, install_wifi_tools, setup_data_directory,
};
use crate::output::println;

/// Qualcomm's vendor id, and the composition the Moxee uses once ADB is on.
///
/// `0xf626` is the factory composition, RNDIS only and no ADB. `0xf622` adds
/// DIAG, serial and ADB, and is what `util moxee-persist-adb` selects. Looking
/// for the second is also how this refuses to run against a device that has
/// not had ADB turned on yet.
const VENDOR_ID: u16 = 0x05c6;
const PRODUCT_ID_WITH_ADB: u16 = 0xf622;

/// The default place a Moxee keeps recordings: its own `/data` is tiny.
const DEFAULT_DATA_DIR: &str = "/cache/rayhunter-data";

struct MoxeeAdbConnection {
    device: ADBUSBDevice,
}

impl DeviceConnection for MoxeeAdbConnection {
    async fn run_command(&mut self, command: &str) -> Result<String> {
        let mut buf = Vec::<u8>::new();
        // The arguments are joined into one command line on the way to the
        // device, so the command has to be quoted or everything after its
        // first word is lost. Found by a root check reading the output of
        // plain `id` when it had asked for `id -u`.
        //
        // Run through a shell so redirection and pipes in the shared setup
        // code behave as they do over telnet. No rootshell wrapper: this ADB
        // is already root.
        let quoted = format!("\"{command}\"");
        self.device
            .shell_command(&["sh", "-c", &quoted], &mut buf)
            .with_context(|| format!("failed running: {command}"))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    async fn write_file(&mut self, path: &str, mut content: &[u8]) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let expected = format!("{:x}", hasher.finalize());

        self.device
            .push(&mut content, &path)
            .with_context(|| format!("failed pushing {path}"))?;

        // Checked rather than assumed. A push that reports success while
        // writing nothing is a real failure mode on these devices, and a
        // truncated daemon starts and dies with no useful message.
        let mut buf = Vec::<u8>::new();
        self.device
            .shell_command(&["sha256sum", path], &mut buf)
            .with_context(|| format!("failed hashing {path} after writing it"))?;
        let output = String::from_utf8_lossy(&buf);
        if !output.contains(&expected) {
            bail!("{path} did not survive the transfer: expected {expected}, device said {output}");
        }
        Ok(())
    }
}

/// Install Rayhunter on a Moxee over ADB.
pub async fn install(
    data_dir: Option<String>,
    reset_config: bool,
    enable_terminal: bool,
) -> Result<()> {
    let data_dir = data_dir.unwrap_or_else(|| DEFAULT_DATA_DIR.to_string());

    println!("Looking for a Moxee with ADB enabled...");
    let device = ADBUSBDevice::new(VENDOR_ID, PRODUCT_ID_WITH_ADB).map_err(|err| {
        anyhow::anyhow!(
            "could not find a Moxee with ADB enabled ({err}).\n\
             \n\
             A Moxee shows as {VENDOR_ID:04x}:{PRODUCT_ID_WITH_ADB:04x} once ADB is on, and as \
             05c6:f626 before that.\n\
             Check with: lsusb | grep 05c6\n\
             \n\
             If it is still f626, ADB has not been turned on yet. That needs the admin password \
             once:\n\
             \n    ./installer util moxee-persist-adb --admin-password 'PASSWORD'\n\
             \n\
             then power cycle the device off USB power."
        )
    })?;

    let mut conn = MoxeeAdbConnection { device };

    // Confirm we really are root before relying on it, rather than finding out
    // halfway through an install.
    let id = conn.run_command("id -u").await?;
    if id.trim() != "0" {
        bail!("expected ADB to be root on a Moxee, but 'id -u' said {id:?}");
    }
    println!("  ADB is root, no password needed.");

    println!("Remounting the root filesystem writable...");
    conn.run_command("mount -o remount,rw /dev/ubi0_0 /")
        .await?;

    println!("Setting up the data directory...");
    setup_data_directory(&mut conn, &data_dir).await?;
    conn.run_command("mkdir -p /data/rayhunter/scripts /data/rayhunter/bin")
        .await?;

    println!("Installing the daemon...");
    conn.write_file(
        "/data/rayhunter/rayhunter-daemon",
        crate::get_file!("FILE_RAYHUNTER_DAEMON"),
    )
    .await?;

    install_wifi_tools(&mut conn).await?;
    install_config(&mut conn, "moxee", reset_config, enable_terminal).await?;

    println!("Installing the startup scripts...");
    conn.write_file(
        "/etc/init.d/rayhunter_daemon",
        RAYHUNTER_DAEMON_INIT.as_bytes(),
    )
    .await?;
    conn.write_file(
        "/etc/init.d/misc-daemon",
        include_bytes!("../../dist/scripts/misc-daemon"),
    )
    .await?;

    conn.run_command("chmod +x /data/rayhunter/rayhunter-daemon")
        .await?;
    conn.run_command("chmod 755 /etc/init.d/rayhunter_daemon")
        .await?;
    conn.run_command("chmod 755 /etc/init.d/misc-daemon")
        .await?;
    conn.run_command("sync").await?;

    println!("Installation complete. Rebooting the device...");
    // The daemon has to be started by init rather than from here: one started
    // from an ADB shell exits as soon as that shell goes away. Seen on this
    // hardware, where it logged a clean startup and then vanished.
    let _ = conn.run_command("reboot").await;

    println!("");
    println!("Once it is back, the web interface is at http://192.168.1.1:8080");
    Ok(())
}
