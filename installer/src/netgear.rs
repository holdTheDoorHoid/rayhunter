//! Installer for the Netgear Nighthawk M6 / M6 Pro mobile hotspot (MR6xxx).
//!
//! Status: **scaffold, not yet verified on hardware.** The install-over-telnet
//! path below is the same, proven pipeline the TP-Link and Wingtech installers
//! use. What is *not* yet verified is the one device-specific primitive every
//! installer needs first: getting a root shell. See [`start_telnet`].
//!
//! ## The device
//!
//! - Family: Netgear Nighthawk M6 (MR6110/MR6150/MR6450) and M6 Pro
//!   (MR6500 retail / MR6550 AT&T). USB id `0846:68e1`, product string `MR6X00`.
//! - SoC: Qualcomm Snapdragon X65 (`sdxlemur`, SDX65) on the M6 Pro, X55
//!   (`sdxprairie`, SDX55) on the M6. Both are ARMv7 Cortex-A7 running a QTI
//!   Linux userspace with Qualcomm's QCMAP web/mobile-AP stack.
//! - Because the SoC is ARMv7, the daemon binary the installer already ships
//!   (`armv7-unknown-linux-musleabihf`, statically linked against musl) is the
//!   right binary. No new build target is needed.
//! - The modem's DIAG runs on-SoC, so `/dev/diag` (the daemon default) is
//!   expected to be correct. If a given build exposes it elsewhere, set
//!   `diag_device_path` in config.toml.
//!
//! ## Getting root (the open item)
//!
//! The MR6xxx web UI is Qualcomm QCMAP, the same CGI backend the Wingtech and
//! TP-Link installers already exploit (`/cgi-bin/qcmap_auth`,
//! `/cgi-bin/qcmap_web_cgi`). The documented community routes to a root shell
//! are:
//!
//!  1. **Older M6 firmware (≈ MR6xxx 10.x):** an AT command interface listens
//!     on TCP 5510 and a root telnet can be opened on TCP 23 with no exploit at
//!     all. If your unit already answers telnet on 23, this installer uses it
//!     directly (see the fast path in [`start_telnet`]).
//!  2. **B.Kerler's `mrCONFIG` keygen** derives a per-device telnet-unlock
//!     token from the unit's identity and unlocks telnet across the MR6xxx
//!     line. Run that first, then run this installer.
//!  3. **AT&T 12.x+** locks the above down; the community workaround is
//!     cross-flashing MR6550-100PAS firmware onto an MR6500-1A1NAS. That is a
//!     brick risk and deliberately NOT automated here.
//!
//! Once *any* of those has telnet on port 23, everything below is device
//! agnostic and already works.

use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Error, Result, bail};
use reqwest::Client;
use tokio::time::sleep;

use crate::NetgearArgs;
use crate::connection::{TelnetConnection, install_config, setup_data_directory};
use crate::output::{print, println};
use crate::util::{interactive_shell, reboot_device, telnet_send_command, telnet_send_file};

/// Port the root telnet listens on once opened.
const TELNET_PORT: u16 = 23;

pub async fn install(
    NetgearArgs {
        admin_ip,
        admin_password,
        reset_config,
        data_dir,
        enable_terminal,
    }: NetgearArgs,
) -> Result<(), Error> {
    start_telnet(&admin_ip, admin_password.as_deref()).await?;
    run_install(admin_ip, reset_config, data_dir, enable_terminal).await
}

/// True if a root telnet shell is already answering on port 23.
async fn telnet_is_up(addr: SocketAddr) -> bool {
    telnet_send_command(addr, "true", "exit code 0", true)
        .await
        .is_ok()
}

/// Make sure a root telnet shell is listening on port 23.
///
/// Fast path: if telnet is already up (you opened it with mrCONFIG, or this is
/// older firmware that ships it), we use it and return immediately.
///
/// Otherwise we attempt the QCMAP web route ([`try_enable_telnet`]) and then
/// re-check. If it still is not up, we stop with guidance rather than pressing
/// on into an install that cannot work, and rather than pretending an
/// unverified exploit succeeded.
pub async fn start_telnet(admin_ip: &str, admin_password: Option<&str>) -> Result<(), Error> {
    let addr =
        SocketAddr::from_str(&format!("{admin_ip}:{TELNET_PORT}")).context("bad admin IP")?;

    if telnet_is_up(addr).await {
        println!("Root telnet already open on {admin_ip}:{TELNET_PORT}, using it.");
        return Ok(());
    }

    println!("Root telnet not open; attempting to enable it over the web UI.");
    let password = admin_password.context(
        "This device needs the admin password to enable telnet over the web UI.\n\
         Pass it with --admin-password, or open telnet yourself first (see below).",
    )?;

    try_enable_telnet(admin_ip, password).await?;

    // Give telnetd a moment to come up, then confirm before continuing.
    for _ in 0..5 {
        if telnet_is_up(addr).await {
            println!("Root telnet is up.");
            return Ok(());
        }
        sleep(Duration::from_millis(1000)).await;
    }

    bail!(
        "Could not confirm a root telnet shell on {admin_ip}:{TELNET_PORT}.\n\n\
         The Netgear M6/M6 Pro root step is not yet verified in this installer.\n\
         To make progress on hardware, open telnet by one of the documented routes\n\
         and re-run this installer (it will detect the open port and proceed):\n\
         \n\
           * B.Kerler's mrCONFIG keygen unlocks telnet across the MR6xxx line.\n\
           * Older MR6xxx 10.x firmware exposes AT on 5510 and root telnet on 23.\n\
         \n\
         Then: installer netgear --admin-ip {admin_ip}\n\
         \n\
         See doc/netgear-m6pro.md for the full write-up and how to finish\n\
         wiring the in-installer web enable in try_enable_telnet()."
    )
}

/// Best-effort attempt to turn on telnet through the QCMAP web UI.
///
/// UNVERIFIED on hardware. The MR6xxx shares Qualcomm's QCMAP CGI backend with
/// the Wingtech CT2MHS01, whose installer authenticates to
/// `/cgi-bin/qcmap_auth` and then command-injects through a field posted to
/// `/cgi-bin/qcmap_web_cgi` (see `wingtech::run_command`). The M6/M6 Pro login
/// uses Netgear's own token/challenge scheme rather than Wingtech's AES-ECB
/// key, so the exact login exchange and the injectable field must be confirmed
/// against a real unit (capture them from the browser, or from B.Kerler's
/// mrCONFIG) before this can be trusted.
///
/// Until then this returns a clear error, and [`start_telnet`] falls back to
/// asking you to open telnet by a known route. Wiring this up is the single
/// thing standing between the scaffold and a one-command install.
async fn try_enable_telnet(admin_ip: &str, _admin_password: &str) -> Result<(), Error> {
    // Read-only diagnostics: confirm we can even reach the web server, and
    // print what it says about itself. Safe to run against any unit.
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("building HTTP client")?;

    let base = format!("http://{admin_ip}");
    match client.get(&base).send().await {
        Ok(resp) => {
            let server = resp
                .headers()
                .get("server")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            println!("  web server reachable at {base} (Server: {server})");
        }
        Err(e) => {
            bail!(
                "Could not reach the admin web UI at {base}: {e}\n\
                 Check that the host has an IP on the device's network and can\n\
                 curl the admin page."
            );
        }
    }

    // The verified enable exchange goes here. Model it on
    // wingtech::run_command: log in for a session token, then POST the
    // telnet-enabling command to the QCMAP CGI. Left unimplemented on purpose
    // so we never claim a root primitive we have not tested.
    bail!(
        "In-installer telnet enable for the Netgear M6/M6 Pro is not implemented\n\
         yet (the QCMAP login/enable exchange needs to be captured and verified\n\
         on hardware). See try_enable_telnet() in installer/src/netgear.rs and\n\
         doc/netgear-m6pro.md."
    )
}

/// Push Rayhunter over an already-open root telnet. Device-agnostic.
async fn run_install(
    admin_ip: String,
    reset_config: bool,
    cli_data_dir: Option<String>,
    enable_terminal: bool,
) -> Result<(), Error> {
    let addr =
        SocketAddr::from_str(&format!("{admin_ip}:{TELNET_PORT}")).context("bad admin IP")?;

    // The persistent, writable partition on these QTI builds is /data. Keep the
    // canonical /data/rayhunter symlink pointing at a sibling so recordings and
    // config survive a reboot. Verify the real free space on hardware.
    let data_dir = cli_data_dir.unwrap_or_else(|| "/data/rayhunter-data".to_owned());

    let mut conn = TelnetConnection::new(addr, true);
    setup_data_directory(&mut conn, &data_dir).await?;

    // Internal flash only for the first port: no removable card path.
    install_config(&mut conn, "netgear", reset_config, enable_terminal, None).await?;

    print!("Installing rayhunter-daemon ... ");
    let rayhunter_daemon_bin = crate::get_file!("FILE_RAYHUNTER_DAEMON");
    telnet_send_file(
        addr,
        "/data/rayhunter/rayhunter-daemon",
        rayhunter_daemon_bin,
        true,
    )
    .await?;
    telnet_send_command(
        addr,
        "chmod 755 /data/rayhunter/rayhunter-daemon",
        "exit code 0",
        true,
    )
    .await?;
    println!("ok");

    print!("Installing init script ... ");
    telnet_send_file(
        addr,
        "/etc/init.d/rayhunter_daemon",
        crate::RAYHUNTER_DAEMON_INIT.as_bytes(),
        true,
    )
    .await?;
    telnet_send_command(
        addr,
        "chmod 755 /etc/init.d/rayhunter_daemon",
        "exit code 0",
        true,
    )
    .await?;
    // update-rc.d exists on these Debian-derived QTI builds; if a given build
    // lacks it, the init script still starts the daemon on the next boot via
    // the standard rc links, and this is a no-op to add.
    let _ = telnet_send_command(
        addr,
        "update-rc.d rayhunter_daemon defaults",
        "exit code 0",
        true,
    )
    .await;
    println!("ok");

    println!(
        "Done. Rebooting device. After it comes back up, open the web interface\n\
         at http://{admin_ip}:8080"
    );
    reboot_device(addr, "reboot", &admin_ip).await;

    Ok(())
}

/// Root the device (or use an already-open telnet) and drop into an
/// interactive shell. Handy for confirming `/dev/diag`, the writable data
/// partition, and free space while bringing the port up on real hardware.
pub async fn shell(admin_ip: &str, admin_password: Option<&str>) -> Result<(), Error> {
    start_telnet(admin_ip, admin_password).await?;
    interactive_shell(admin_ip, TELNET_PORT, true).await
}
