use std::future::Future;
use std::net::SocketAddr;

use anyhow::{Result, bail};

use crate::output::{print, println};

/// Abstraction for device communication (telnet or ADB)
pub trait DeviceConnection {
    /// Run a shell command and return its output
    fn run_command(&mut self, command: &str) -> impl Future<Output = Result<String>> + Send;

    /// Write a file to the device
    fn write_file(&mut self, path: &str, content: &[u8])
    -> impl Future<Output = Result<()>> + Send;
}

/// Check if a file exists using a DeviceConnection
pub async fn file_exists<C: DeviceConnection>(conn: &mut C, path: &str) -> bool {
    conn.run_command(&format!("test -f '{path}' && echo exists || echo missing"))
        .await
        .map(|output| output.contains("exists"))
        .unwrap_or(false)
}

/// Set a top-level boolean key in a TOML document, preserving everything else.
///
/// Appending the key would be wrong. The config template ends inside the
/// `[analyzers]` table, so a key added at the end of the file is parsed as
/// `analyzers.<key>` rather than a top-level setting — and since that table
/// ignores keys it does not know, it is dropped without any error at all. A
/// top-level key has to go before the first table header.
fn set_top_level_bool(config: &str, key: &str, value: bool) -> String {
    let assignment = format!("{key} = {value}");
    let mut lines: Vec<String> = config.lines().map(str::to_string).collect();

    // Only look above the first table header: a matching name below one belongs
    // to that table, not to the document.
    let end = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());

    let existing = lines[..end].iter().position(|line| {
        let trimmed = line.trim_start();
        // A commented-out example is not the setting.
        !trimmed.starts_with('#')
            && trimmed
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
    });

    match existing {
        Some(at) => lines[at] = assignment,
        None => lines.insert(end, assignment),
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Shared config installation logic. Installs to /data/rayhunter/config.toml which resolves
/// through the symlink to the actual data directory.
pub async fn install_config<C: DeviceConnection>(
    conn: &mut C,
    device_type: &str,
    reset_config: bool,
    enable_terminal: bool,
) -> Result<()> {
    let config_path = "/data/rayhunter/config.toml";
    if reset_config || !file_exists(conn, config_path).await {
        let config = crate::CONFIG_TOML.replace(
            r#"#device = "orbic""#,
            &format!(r#"device = "{device_type}""#),
        );
        // Only settable here, never from the web interface. The daemon runs as
        // root, so the terminal is the difference between an interface that
        // reads data and one that can do anything at all; turning it on should
        // take physical access to the device.
        let config = set_top_level_bool(&config, "terminal_enabled", enable_terminal);
        conn.write_file(config_path, config.as_bytes()).await?;
    } else {
        println!("Config file already exists, skipping (use --reset-config to overwrite)");
        // The terminal is the exception to keeping the existing config: flashing
        // is the only place it can be turned on, so the flag given here has to
        // take effect without also demanding the rest of somebody's settings be
        // thrown away. Absent, it turns the terminal off — whether it is on
        // should always be what was asked for at the last flash.
        let existing = conn.run_command(&format!("cat '{config_path}'")).await?;
        let updated = set_top_level_bool(&existing, "terminal_enabled", enable_terminal);
        if updated != existing {
            conn.write_file(config_path, updated.as_bytes()).await?;
            println!(
                "  the web terminal is now {} on this device.",
                if enable_terminal {
                    "ENABLED"
                } else {
                    "disabled"
                }
            );
        }
    }
    if enable_terminal {
        println!(
            "  the web terminal is ENABLED on this device. Anyone who can reach the web\n  interface can run commands as root. Set a password under Configuration."
        );
    }
    Ok(())
}

/// Install wifi tools (wpa_supplicant, wpa_cli, iw) to /data/rayhunter/bin.
///
/// Skips any binary that is already present on the device (e.g. provided by firmware),
/// since those may be newer or better-integrated than the bundled versions.
///
/// In debug builds the wpa-supplicant binaries may not be bundled (build.rs sets the
/// env vars to empty in that case); when so, this is a no-op so devs don't have to
/// build wpa-supplicant just to install on Orbic.
pub async fn install_wifi_tools<C: DeviceConnection>(conn: &mut C) -> Result<()> {
    if env!("FILE_WPA_SUPPLICANT").is_empty() {
        println!("wifi tools were not built into this installer, skipping");
        return Ok(());
    }
    let tools: &[(&str, &str, &[u8])] = &[
        (
            "wpa_supplicant",
            "/data/rayhunter/bin/wpa_supplicant",
            crate::get_file!("FILE_WPA_SUPPLICANT"),
        ),
        (
            "wpa_cli",
            "/data/rayhunter/bin/wpa_cli",
            crate::get_file!("FILE_WPA_CLI"),
        ),
        ("iw", "/data/rayhunter/bin/iw", crate::get_file!("FILE_IW")),
    ];
    for &(name, dest, payload) in tools {
        if device_has_binary(conn, name).await {
            println!("{name} already on device, skipping");
        } else {
            conn.write_file(dest, payload).await?;
            conn.run_command(&format!("chmod +x {dest}")).await?;
        }
    }
    Ok(())
}

async fn device_has_binary<C: DeviceConnection>(conn: &mut C, name: &str) -> bool {
    // `command -v` is a POSIX shell builtin, so it works on minimal busybox firmware
    // even when /usr/bin/which is absent.
    conn.run_command(&format!(
        "\"command -v {name} >/dev/null 2>&1 && echo FOUND || echo MISSING\""
    ))
    .await
    .map(|out| out.contains("FOUND"))
    .unwrap_or(false)
}

/// Check if a directory exists using a DeviceConnection
pub async fn dir_exists<C: DeviceConnection>(conn: &mut C, path: &str) -> bool {
    conn.run_command(&format!("test -d '{path}' && echo exists || echo missing"))
        .await
        .map(|output| output.contains("exists"))
        .unwrap_or(false)
}

/// Check if a path is a symlink using a DeviceConnection
pub async fn is_symlink<C: DeviceConnection>(conn: &mut C, path: &str) -> bool {
    conn.run_command(&format!("test -L '{path}' && echo yes || echo no"))
        .await
        .map(|output| output.contains("yes"))
        .unwrap_or(false)
}

/// Read the target of a symlink using a DeviceConnection
pub async fn readlink<C: DeviceConnection>(conn: &mut C, path: &str) -> Result<String> {
    // Use a prefix marker to find the actual output line, since some shells (TP-Link) echo
    // back the command and run_command appends protocol lines.
    let output = conn
        .run_command(&format!("echo RL:$(readlink '{path}')"))
        .await?;

    for line in output.lines() {
        if let Some(target) = line.trim().strip_prefix("RL:") {
            return Ok(target.to_string());
        }
    }

    bail!("unexpected readlink output: {output:?}");
}

/// Set up the data directory at `data_dir` and create a symlink from `/data/rayhunter` to it.
///
/// Handles migration from old locations:
/// - If `/data/rayhunter` is a real directory, moves its contents to `data_dir`
/// - If `/data/rayhunter` is a symlink to a different location, moves from the old target
/// - If `/data/rayhunter` doesn't exist, just creates the symlink
/// - If `/data/rayhunter` is a symlink to `data_dir`, does nothing
pub async fn setup_data_directory<C: DeviceConnection>(conn: &mut C, data_dir: &str) -> Result<()> {
    if data_dir == "/data/rayhunter" {
        bail!("data_dir must not be /data/rayhunter");
    }

    if data_dir.contains("'") {
        bail!("data_dir must not contain an apostrophe (')");
    }

    // Determine where old data lives, if anywhere
    let old_data_source = if is_symlink(conn, "/data/rayhunter").await {
        let current_target = readlink(conn, "/data/rayhunter").await?;
        if current_target == data_dir {
            println!("Data directory already configured at {data_dir}");
            return Ok(());
        }
        conn.run_command("rm -f /data/rayhunter").await?;
        // The old symlink target is where data actually lives
        if dir_exists(conn, &current_target).await {
            Some(current_target)
        } else {
            None
        }
    } else if dir_exists(conn, "/data/rayhunter").await {
        if dir_exists(conn, data_dir).await {
            bail!("Both /data/rayhunter and {data_dir} exist and are directories.");
        }
        // Real directory (pre-migration Orbic state)
        Some("/data/rayhunter".to_string())
    } else {
        None
    };

    // Migrate old data if present
    if let Some(old_source) = &old_data_source {
        // Stop rayhunter-daemon so it doesn't write during migration.
        // The device will be rebooted at the end of installation anyway.
        print!("Stopping rayhunter-daemon ... ");
        let _ = conn
            .run_command("/etc/init.d/rayhunter_daemon stop 2>/dev/null; true")
            .await;
        println!("ok");

        print!("Migrating data from {old_source} to {data_dir} ... ");

        // mv old data into its place. If source and destination are on the same filesystem,
        // this is an instant rename.
        // XXX: DeviceConnection::run_command does not expose the exit code of the ran command. It
        // probably should, or a utility for it should exist?
        let mv_output = conn
            .run_command(&format!("mv '{old_source}' '{data_dir}' && echo MV_OK"))
            .await?;
        if mv_output.contains("MV_OK") {
            println!("ok");
        } else {
            bail!("Failed to move data from {old_source} to {data_dir}:\n{mv_output}");
        }
    } else {
        // No migration needed, just ensure the target directory exists
        conn.run_command(&format!("mkdir -p '{data_dir}'")).await?;
    }

    // Create the symlink
    print!("Creating symlink /data/rayhunter -> {data_dir} ... ");
    conn.run_command("mkdir -p /data").await?;
    conn.run_command(&format!("ln -sf '{data_dir}' /data/rayhunter"))
        .await?;
    println!("ok");

    Ok(())
}

/// Telnet-based connection wrapper
pub struct TelnetConnection {
    pub addr: SocketAddr,
    pub wait_for_prompt: bool,
}

impl TelnetConnection {
    pub fn new(addr: SocketAddr, wait_for_prompt: bool) -> Self {
        Self {
            addr,
            wait_for_prompt,
        }
    }
}

impl DeviceConnection for TelnetConnection {
    async fn run_command(&mut self, command: &str) -> Result<String> {
        crate::util::telnet_send_command_with_output(
            self.addr,
            command,
            self.wait_for_prompt,
            std::time::Duration::from_secs(10),
        )
        .await
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<()> {
        crate::util::telnet_send_file(self.addr, path, content, self.wait_for_prompt).await
    }
}

#[cfg(test)]
mod tests {
    use super::set_top_level_bool;

    /// The bug this exists to prevent: the template ends inside `[analyzers]`,
    /// so a key appended to the end is silently swallowed by that table and the
    /// setting never takes effect.
    #[test]
    fn the_key_goes_above_the_first_table() {
        let config = "port = 8080\n\n[analyzers]\nnull_cipher = true\n";
        let out = set_top_level_bool(config, "terminal_enabled", true);
        let parsed: toml::Value = toml::from_str(&out).expect("still valid TOML");
        assert_eq!(parsed["terminal_enabled"].as_bool(), Some(true));
        assert!(
            parsed["analyzers"].get("terminal_enabled").is_none(),
            "leaked into the analyzers table: {out}"
        );
    }

    #[test]
    fn an_existing_value_is_replaced_not_duplicated() {
        let config = "port = 8080\nterminal_enabled = false\n\n[analyzers]\n";
        let out = set_top_level_bool(config, "terminal_enabled", true);
        assert_eq!(out.matches("terminal_enabled").count(), 1, "{out}");
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(parsed["terminal_enabled"].as_bool(), Some(true));
    }

    /// Flashing without the flag has to turn the terminal off again.
    #[test]
    fn it_can_turn_the_setting_off() {
        let config = "terminal_enabled = true\n\n[analyzers]\n";
        let out = set_top_level_bool(config, "terminal_enabled", false);
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(parsed["terminal_enabled"].as_bool(), Some(false));
    }

    /// A commented-out example must not be mistaken for the real setting, or it
    /// would be rewritten into a live one in the wrong place.
    #[test]
    fn a_commented_example_is_not_the_setting() {
        let config = "# terminal_enabled = true\nport = 8080\n\n[analyzers]\n";
        let out = set_top_level_bool(config, "terminal_enabled", true);
        assert!(out.contains("# terminal_enabled = true"), "{out}");
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(parsed["terminal_enabled"].as_bool(), Some(true));
    }

    /// Comments and ordering elsewhere are somebody's own config; leave them be.
    #[test]
    fn the_rest_of_the_file_is_untouched() {
        let config = "# a note\nport = 8080\n\n[analyzers]\nnull_cipher = true\n";
        let out = set_top_level_bool(config, "terminal_enabled", false);
        assert!(out.contains("# a note"));
        assert!(out.contains("null_cipher = true"));
    }

    /// The real template, which is what actually ships.
    #[test]
    fn the_shipped_template_gets_a_top_level_key() {
        let out = set_top_level_bool(crate::CONFIG_TOML, "terminal_enabled", true);
        let parsed: toml::Value = toml::from_str(&out).expect("template stays valid");
        assert_eq!(parsed["terminal_enabled"].as_bool(), Some(true));
    }
}
