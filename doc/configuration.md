# Configuration

Rayhunter can be configured through web user interface or by editing `/data/rayhunter/config.toml` on the device.

![rayhunter_config](./rayhunter_config.png)

Through web UI you can set:
- **Device UI Level**, which defines what Rayhunter shows on device's built-in screen. *Device UI Level* could be:
  - *Invisible mode*: Rayhunter does not show anything on the built-in screen
  - *Subtle mode (colored line)*: Rayhunter shows green line if there are no warnings, red line if there are warnings (warnings could be checked through web UI) and white line if Rayhunter is not recording.
  - *Demo mode (orca gif)*, which shows image of orcas *and* colored line.
  - *EFF logo*, which shows EFF logo *and* colored line.
  - *High visibility (full screen color)*: fills the entire screen with the status color (green for recording, red for warnings, white for paused).
  - *Custom GIF*: plays an animation you upload for each state; see *Device Display GIFs* below.
- **Device Input Mode**, which defines behavior of built-in power button of the device. *Device Input Mode* could be:
  - *Disable button control*: built-in power button of the device is not used by Rayhunter.
  - *Double-tap power button to start new recording*: double clicking on a built-in power button of the device stops and immediately restarts the recording. This could be useful if Rayhunter's heuristics is triggered and you get the red line, and you want to "reset" the past warnings. Normally you can do that through web UI, but sometimes it is easier to double tap on power button.
- **Colorblind Mode** enables color blind mode: on the device's own display, the status line is drawn blue instead of green for recording and informational events. Warning colors are unchanged (yellow for low, orange for medium, red for high), but warning severity is additionally conveyed by line pattern, dotted for low, dashed for medium, solid for high, which remains readable regardless of color perception. Note that this setting affects only the device's display, not the colors used in the web UI. Please note that this does not cover all types of color blindness, but switching green to blue should be about enough to differentiate the color change for most types of color blindness.
- **Device Display Colors** lets you choose your own color for each state of the device's status line. There is a color picker for each of *Paused*, *Recording*, *Low warning*, *Medium warning* and *High warning*, alongside a preview of how that line will be drawn on the device. Any state you leave alone keeps Rayhunter's built-in color, including the green-to-blue substitution made by *Colorblind Mode*; a color you pick explicitly takes precedence over that substitution. Line patterns (dotted for low, dashed for medium, solid for high) are not configurable, so warning severity stays distinguishable whatever colors you choose. These colors apply only to the device's own display, they do not change the web UI, and they have no effect on devices with a one-bit display such as the TP-Link M7350, which draw status icons rather than a colored line. In `config.toml` these live under a `[display_colors]` table, with each key set to an `#rrggbb` string:

  ```toml
  [display_colors]
  recording = "#00b4ff"
  warning_high = "#ff00ff"
  ```

  Omit a key to keep the built-in color for that state. An unparseable value is ignored (a warning is written to the log) and the built-in color is used instead.

  In the web UI these live in a **Status Line** section shown for every UI level except *Invisible*, since a colored status line is drawn in all of them, as a thin line over the image in *Demo mode* and *EFF logo*, filling the screen in *High visibility*, and for any state without a GIF in *Custom GIF*. The *Colorblind Mode* checkbox sits in this section too, directly above the colors it changes.

  The section warns (without blocking) when a chosen color is so dark it would be near-invisible on the device's black screen, when two warning severities are too similar to tell apart at a glance, and when colorblind mode is enabled but a custom *Recording* color is overriding it.
- **Line height** sets how tall the status line is, from 1 pixel up to the full height of the display, and appears when the UI level is *Subtle mode*. It is stored as `status_bar_height`:

  ```toml
  status_bar_height = 48
  ```

  Omit it for the built-in 2 pixels. The value is clamped to the display's height when drawn, so a config copied between devices with different screens cannot produce a broken display. *High visibility* ignores it and always fills the screen, setting the slider to its maximum in *Subtle mode* produces the same result.
- **Device Display GIFs** appear when *Device UI Level* is set to *Custom GIF*, and let you upload your own animation for each of the five display states. GIFs are uploaded with `POST /api/display-gif/{state}` (state being one of `paused`, `recording`, `warning_low`, `warning_medium`, `warning_high`) and stored in `gif_store_path` (default `/data/rayhunter/gifs`) as `<state>.gif`; `GET /api/display-gif/{state}` serves a stored GIF back, which is how the web UI previews what is currently on the device; `POST /api/display-gif/{state}/delete` removes one. Uploading stores the file immediately but does not restart Rayhunter, the change applies when the configuration is next saved, so several GIFs can be uploaded in one go. The `[display_gifs]` table records which states have a GIF:

  ```toml
  [display_gifs]
  recording = "recording.gif"
  warning_high = "warning_high.gif"
  ```

  A state with no GIF falls back to drawing its colored status line, so the display is never blank. Uploads must be GIFs (checked by header) and at most 2MB. Note the device screen is small, 128x128 on the Orbic RC400L, and larger GIFs are scaled down on the device, so authoring at the native size gives the best result. Frames are decoded one at a time during playback rather than all at once, which keeps memory use flat regardless of how long the animation is. A change of state interrupts a playing GIF between frames, so a long animation never delays a warning from being shown.
- **Enable the demo warning button** (`demo_mode`) adds a control to the main page that injects a synthetic warning, for demonstrating Rayhunter to an audience. The injected message is a NAS Security Mode Command selecting the null cipher, which is one of the clearest signs of a fake base station. It is fed into the diag stream ahead of analysis, so it is written to the recording and passes through the real heuristics: the warning appears in the history and the device turns red exactly as it would for a genuine detection.

  Every event it produces is prefixed `[DEMO, NOT REAL]` in the analysis file, so a demo warning can be recognised later by somebody who was not present. Even so, **do not treat a recording containing demo data as evidence, or send one to EFF**. The setting is off by default and `POST /api/demo-warning` is refused with 403 while it is off, and with 503 when no recording is running.
- **Clock Sync** controls what happens when Rayhunter's clock drifts from the clock of the browser you're viewing the web UI with. Some devices have no battery-backed real-time clock, so they lose the time whenever they reboot, and an incorrect clock means incorrect timestamps on your recordings. The modes are:
  - *Prompt (ask before syncing)*, the default, shows a warning with both clocks and lets you decide whether to copy the browser's time to the device.
  - *Autosync (copy browser clock automatically)* silently corrects the device clock whenever you open the web UI and the difference is more than 30 seconds. Useful for devices that reset their clock on every reboot.
  - *Off (never warn or sync)* disables both the warning and any automatic correction.

  Note that the correction is an offset held in memory only: it is **not** written to the device's clock and is lost when the daemon restarts, so with *Autosync* it is re-applied each time you load the web UI.
- **Diagnostic device path** (`diag_device_path`) says where to open the modem's diagnostic character device. There is no setting for it in the web interface, deliberately: an incorrect value stops Rayhunter recording anything at all, and it is not something anyone should reach for while looking around the settings page. Edit `config.toml` on the device instead:

  ```toml
  diag_device_path = "/dev/mhi_DIAG"
  ```

  Leave it out and Rayhunter uses `/dev/diag`, which is correct on every device it currently supports. It exists for hardware where the modem sits behind MHI or PCIe and the node lives somewhere else entirely, which is otherwise the only thing standing between those devices and working. A blank value is treated as unset rather than as a path, and if the device cannot be opened the error names the path it tried, so a typo says so plainly.
- **Switching WiFi off from the buttons** (`wifi_ap_button_toggle`, under *Network*) lets a burst of button presses on the device switch off its own WiFi access point. A hotspot running as a sensor does not need to be broadcasting a network, and not broadcasting one saves power and draws less attention.

  It is off by default, because the access point is also how most people reach this interface. When on, the gesture is several presses in quick succession: `wifi_ap_toggle_presses` (5) within `wifi_ap_toggle_window_secs` (4). Fewer than four presses is never accepted, whatever the setting says, because a double tap is something a pocket can produce and is already used for starting a new recording.

  `wifi_ap_off_mode` decides how it comes back. `temporary` brings it back on its own after `wifi_ap_off_minutes`; `until_restart` leaves it off until the device is restarted.

  **Restarting the device always brings WiFi back**, whatever these are set to. That is not something Rayhunter arranges, it is how the device behaves: the firmware starts the access point when it boots. So this cannot lock you out: the worst case is a power cycle, which needs no cable, no menu and no password. Doing the gesture again while WiFi is off also brings it back, by restarting, because on the hardware this was measured on restarting is the only thing that reliably works. That means bringing WiFi back interrupts recording for about half a minute; switching it off does not.

  Devices whose WiFi is run by something Rayhunter does not know how to stop report the setting as unavailable in the log rather than pretending to offer it.
- **USB debugging (ADB)** (`adb_enabled`, under *Network*) turns ADB on or off. It appears only on devices where Rayhunter knows how to change it safely, and takes effect at the next restart, because the USB mode is chosen when the device boots.

  Leaving it unset leaves the device exactly as its installer left it, which is why a device that already has ADB keeps it. Setting it writes the value for the USB mode that includes ADB.

  **ADB on these devices runs as root.** Anyone who can plug a cable in gets complete control of the device, without needing the web interface, the WiFi password or anything else. Useful while installing over USB or debugging; worth turning off before carrying the device anywhere.

  The control is hidden on devices whose USB mode is chosen by a value that has not been checked on hardware. Devices pick their mode from a number in a file, and the numbers mean different things on different hardware: a Moxee uses one value for the mode including ADB, while an Orbic has a different value in the same file for a mode that also includes ADB. Writing the wrong number selects a mode nobody has tried, and getting that wrong takes the device off USB entirely, which is the one failure that needs a cable to fix. So Rayhunter only ever changes a value it recognises.
- **Automatically check for software updates** enables periodic checks against the Rayhunter GitHub releases page. When a newer release is found, the web UI shows a notice and, if ntfy update notifications are enabled, a notification is sent.
- **ntfy URL**, which allows setting a [ntfy](https://ntfy.sh/) URL to which notifications of new detections will be sent. The topic should be unique to your device, e.g., `https://ntfy.sh/rayhunter_notifications_ba9di7ie` or `https://myserver.example.com/rayhunter_notifications_ba9di7ie`. The ntfy Android and iOS apps can then be used to receive notifications. More information can be found in the [ntfy docs](https://docs.ntfy.sh/).
- **Enabled Notification Types** allows enabling or disabling the following types of notifications:
  - *Warnings*, which will alert when a heuristic is triggered. Alerts will be sent at most once every five minutes.
  - *Low Battery*, which will alert when the device's battery is low. Notifications may not be supported for all devices, you can check if your device is supported by looking at whether the battery level indicator is functioning on the System Information section of the Rayhunter UI.
  - *Software Updates*, which will alert when a new Rayhunter release is available. Only triggers when *Automatically check for software updates* is enabled.
- With **Analyzer Heuristic Settings** you can switch on or off built-in [Rayhunter heuristics](heuristics.md). Some heuristics are experimental or can trigger a lot of false positive warnings in some networks (our tests have shown that some heuristics have different behavior in US or European networks). In that case you can decide whether you would like to have the heuristics that trigger a lot of false positives on or off. Please note that we are constantly improving and adding new heuristics, so a new release may reduce false positives in existing heuristics as well.

## GPS

The **GPS Settings** allows you to attach GPS-based location history to every recording. Data is stored as a separate JSON file next to QMDL, and also inlined into the PCAP file as packet comment.

The modes are:

- *Disabled*, the default option, disables this feature entirely.

- *Fixed*, for hardcoding latitude (-90 to 90) and longitude (-180 to 180) for devices that don't move very often or at all. Every packet in the recording will have that location.

- *API Endpoint*, enables the `POST /api/gps` endpoint so that third-party tools (i.e. your own scripts) can update location info continuously. Please refer to the [API documentation](api-docs.md) for more info.

The GPS data is stored as a separate JSON file next to QMDL captures, and contains its own timestamps. These timestamps are meant to be compared during analysis with the packet timestamp so we know the time difference between the packet capture from the GPS capture, if there is any, since GPS data and packet data may come from two entirely separate devices.

## WiFi Client Mode

On the **Orbic**, **Moxee**, **UZ801**, **TMOHS1**, and **Wingtech**, Rayhunter can connect the device to an existing WiFi network while keeping the hotspot running. This gives the device internet access for [notifications](https://docs.ntfy.sh/) and lets you reach the web UI from any device on that network.

- **Enable WiFi** turns WiFi client mode on or off. Disabling it does not erase saved credentials.
- **Scan** searches for nearby networks. Select one from the dropdown, or type an SSID manually.
- **Password** is required for WPA/WPA2 networks. The password is stored separately from `config.toml` (in `wpa_sta.conf` on the device) and is never exposed through the API.
- **DNS Servers** lets you override the DNS servers used when connected. Defaults to `9.9.9.9` and `149.112.112.112` (Quad9) if not set.

After saving, the connection status will show **connecting**, **connected** (with the assigned IP address), or **failed** (with an error message). If the connection fails, check that the SSID and password are correct and that the network is in range.

### Crash Recovery

The WiFi kernel module (`wlan.ko`) can occasionally crash or unload, taking both the hotspot and client interfaces down with it. Rayhunter includes a watchdog that detects this and automatically reloads the module, restarts the hotspot, and reconnects to the configured network. During recovery the WiFi status will show **recovering**.

On the first detection of a crash, a diagnostic snapshot is saved to `/data/rayhunter/crash-logs/` on the device. You can pull these logs with `adb pull /data/rayhunter/crash-logs/` and inspect them to understand what went wrong. Each log contains:

- **dmesg** output (kernel messages). Look for backtraces, `BUG:`/`Oops:` lines, or `wlan`/`wcnss` errors. The kernel ring buffer is small and gets overwritten quickly, so crash details may already be gone if the crash happened well before detection.
- **/proc/modules** snapshot. If `wlan` is absent, the module fully unloaded. If present but interfaces are gone, the driver is stuck.
- **ip addr** output confirming which network interfaces existed at snapshot time.
- **ps** output showing which WiFi-related processes (`hostapd`, `wpa_supplicant`, `wland`) were still running.

If recovery fails after 5 attempts, the status will change to **failed**. A reboot of the device will reset WiFi.

## WebDAV Upload

Rayhunter can automatically upload finished recordings to a WebDAV server. When a `[webdav]` section is present in `config.toml`, a background worker periodically scans the recording store and uploads any closed entry that is older than `min_age_secs`. Each eligible entry uploads two files: the raw `.qmdl` capture and its `.ndjson` analysis output. After a successful upload the entry is either marked as uploaded in the manifest (and skipped on subsequent polls), or deleted locally if `delete_on_upload = true`. With no `[webdav]` section, no upload worker runs.

WebDAV upload is currently configurable only by editing `config.toml`, there is no web UI control for it yet.

| Key | Required | Default | Description |
| --- | --- | --- | --- |
| `url` | yes |, | WebDAV server base URL, e.g. `https://example.com/remote.php/files/user/rayhunter/` |
| `username` | no |, | HTTP Basic auth username |
| `password` | no |, | HTTP Basic auth password |
| `upload_timeout_secs` | no | `300` | Timeout (seconds) for each upload request |
| `poll_interval_secs` | no | `3600` | How often (seconds) the worker scans for eligible entries |
| `min_age_secs` | no | `86400` | Minimum age (seconds) an entry must have before it becomes eligible for upload |
| `delete_on_upload` | no | `false` | Delete the entry locally after a successful upload |

Example:

```toml
[webdav]
url = "https://dav.example.com/rayhunter/"
username = "user"
password = "pass"
upload_timeout_secs = 300
poll_interval_secs = 3600
min_age_secs = 86400
delete_on_upload = false
```

A few notes on behavior:

- **Auth:** HTTP Basic. Supplying a `password` without a `username` is rejected, the request is sent unauthenticated and a warning is logged.
- **Retries and overwrites:** each entry's two files (`.qmdl` and `.ndjson`) must both upload successfully before the entry is marked as uploaded in the manifest. If one upload fails, the entry stays unmarked and both files are retried on the next poll, the one that previously succeeded will be overwritten on the server. Once an entry is marked as uploaded, Rayhunter will not upload it again.
- **Currently-recording entry:** the active recording is never uploaded; only closed entries are eligible.

If you prefer editing `config.toml` file, you need to obtain a shell on your [Orbic](./orbic.md#obtaining-a-shell) or [TP-Link](./tplink-m7350.md#obtaining-a-shell) device and edit the file manually. You can view the [default configuration file on GitHub](https://github.com/EFForg/rayhunter/blob/main/dist/config.toml.in).
