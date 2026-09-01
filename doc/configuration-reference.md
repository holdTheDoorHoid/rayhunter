# Configuration Reference

This is the exhaustive list of every setting Rayhunter reads from its
configuration file, `config.toml`. For *how* to change settings — through the web
interface or by editing the file — see [Configuration](./configuration.md); this
page is the reference you check when you need a specific key's type, default, and
effect.

## How the file is read, and three traps

Rayhunter parses `config.toml` forgivingly, which has consequences worth knowing
before you edit it by hand:

- **A missing key takes its default.** This is the upgrade path: a config written
  by an older version comes up with any newer settings at their defaults, and
  detectors absent from the file come up **on**.
- **A misspelled key is silently ignored.** Because unknown keys are dropped
  without error, a typo does not fail — the setting never takes effect at all.
  Check your spelling against this page.
- **A top-level key placed after the `[analyzers]` table lands *inside* it.** The
  shipped template ends with `[analyzers]`, so a plain key added at the bottom is
  read as an analyzer setting and dropped. Put top-level keys above the first
  `[table]` header.

Two more behaviours that surprise people:

- **Saving the config through the web interface restarts the daemon**, which
  takes about a minute before the interface responds again.
- **Saving replaces the whole file.** Editing one setting in the interface and
  saving rewrites everything from what the running version understands, so a
  setting that version does not know is dropped. This matters when moving between
  the fork and upstream — see [Compatibility With Upstream](./fork/compatibility.md).

In the tables below, **Fork** marks a setting this fork adds that upstream does
not have, and **(not in template)** marks a key absent from the shipped
`config.toml.in`, so it exists but is not shown there.

## Core and storage

| Key | Type | Default | Effect and what breaks |
|---|---|---|---|
| `qmdl_store_path` | string | `/data/rayhunter/qmdl` | Where recordings are stored. A path on a read-only or missing mount means no recordings. |
| `port` | integer | `8080` | Web interface port. The daemon fails to start if it cannot bind this port. |
| `debug_mode` | boolean | `false` | Runs without the diagnostic thread, display, or recording; the store must already exist. For development, not normal use. |
| `device` | enum | `orbic` | Selects the display driver and device-specific paths. Written by the installer. A wrong value gives a broken display and wrong system paths. **(not in template)** |

## The device display

| Key | Type | Default | Effect |
|---|---|---|---|
| `ui_level` | integer | `1` | Display mode: 0 invisible, 1 subtle status line, 2 orca demo animation, 3 EFF logo, 4 fill the screen with the status colour, 5 custom images (**Fork**), 128 trans-flag colours. |
| `colorblind_mode` | boolean | `false` | Substitutes blue for green on the status line. |
| `display_colors.*` | hex strings | unset | Per-state colour overrides (`paused`, `recording`, `warning_low`, `warning_medium`, `warning_high`), each `#rrggbb`. A malformed value falls back to the built-in colour rather than breaking the display. Colour displays only. **Fork, (not in template)** |
| `status_bar_height` | integer | unset (2px) | Height of the status line in pixels, clamped to the screen height at draw time. Ignored by the fill-the-screen mode. **Fork, (not in template)** |
| `display_gifs.*` | strings | unset | Per-state uploaded image filenames, used when `ui_level` is 5. The image itself lives under `gif_store_path`; setting only the file without this key shows nothing. **Fork, (not in template)** |
| `gif_store_path` | string | `/data/rayhunter/gifs` | Where uploaded display images are stored. **Fork, (not in template)** |
| `keep_screen_on` | integer | `0` | Stop the screen blanking: 0 never, 1 always, 2 only while plugged in. Implemented on the Orbic; other devices ignore it. **Fork, (not in template)** |
| `pause_display_on_keypress` | boolean | `true` | A button press shrinks the overlay to the thin status line for twenty seconds, so the device's own screens (including the WiFi password) can be read. **Fork, (not in template)** |
| `key_input_mode` | integer | `0` | 0 ignores buttons; 1 makes a double-tap of the power button start a new recording. |

## Detection

| Key | Type | Default | Effect |
|---|---|---|---|
| `[analyzers]` table | booleans | all on except `test_analyzer` | Enables each detector. Keys: `imsi_requested`, `connection_redirect_2g_downgrade`, `lte_sib6_and_7_downgrade`, `null_cipher`, `nas_null_cipher`, `incomplete_sib`, `lpp_location_request` (**Fork**), `lpp_location_tracking` (**Fork**), `rrlp_location_request` (**Fork**), `diagnostic_analyzer`, `test_analyzer`. Missing keys default on (except `test_analyzer`). See the [Detector Reference](./detectors/index.md). |
| `demo_mode` | boolean | `false` | Enables the demo warning button, which injects a clearly-labelled synthetic warning into the current recording. See [Your First Warning](./first-warning.md). **Fork, (not in template)** |
| `show_subscriber_identity` | boolean | `false` | Whether the web interface discloses this device's own IMSI, IMEI and temporary identity. Off by default because the interface may be unauthenticated. **Fork, (not in template)** |
| `terminal_enabled` | boolean | `false` | Whether the web interface may run a command on the device. Only settable at install time (`--enable-terminal`), never from the interface. **Fork, (not in template)** |

## Recording management

| Key | Type | Default | Effect |
|---|---|---|---|
| `min_space_to_start_recording_mb` | integer | `1` | Below this much free space, a recording will not start. |
| `min_space_to_continue_recording_mb` | integer | `1` | Below this much free space, a running recording stops. |
| `auto_delete_clean_recordings` | boolean | `false` | When space runs low, delete analysed recordings that raised no warning, oldest first. Never deletes a named recording, the current one, or one still pending upload. **Fork, (not in template)** |
| `max_recording_size_mb` | integer | unset | Start a fresh recording once the current one reaches this size. Keeps files small enough to download. **Fork, (not in template)** |
| `max_recording_minutes` | integer | unset | Start a fresh recording once the current one has run this long. With both set, whichever comes first wins. **Fork, (not in template)** |

## Notifications and updates

| Key | Type | Default | Effect |
|---|---|---|---|
| `ntfy_url` | string | unset | If set, sends a push notification to this ntfy URL on a new warning. |
| `enabled_notifications` | list | `["Warning", "LowBattery"]` | Which notification types fire (does nothing without `ntfy_url`). |
| `auto_check_updates` | boolean | see note | Periodically check GitHub for new releases and show a notice. **Note:** the code default is `true`, but the shipped template sets `false`; a config missing the key gets update checks. Which is intended is [an open question](./INVENTORY.md). |
| `clock_sync_mode` | integer | `2` | Clock-drift handling: 0 off, 1 autosync silently, 2 prompt. The offset is kept in memory only and lost on restart. |

## Network and WiFi client mode

| Key | Type | Default | Effect |
|---|---|---|---|
| `wifi_enabled` | boolean | `false` | Connect the device to an existing WiFi network (client mode). |
| `wifi_ssid` / `wifi_password` / `wifi_security` | strings | unset | Client-mode credentials. Managed through the interface; the real credentials live in `wpa_sta.conf`, and these fields are overwritten from it at boot (the password never round-trips). **(not in template)** |
| `dns_servers` | list | unset (Quad9) | DNS servers to use in client mode. Defaults to Quad9 if unset. |

## GPS

| Key | Type | Default | Effect |
|---|---|---|---|
| `gps_mode` | integer | `0` | 0 off, 1 fixed coordinates, 2 fed through the API. When off, `/api/gps` returns 404 (normal; the interface tolerates it). **(not in template)** |
| `gps_fixed_latitude` / `gps_fixed_longitude` | number | unset | Used when `gps_mode` is 1. A missing pair logs a warning and records no coordinates. **(not in template)** |

## WebDAV upload

Present only if a `[webdav]` section exists; the upload worker runs whenever
`webdav.url` is non-empty. Finished recordings (the raw capture and its analysis)
are uploaded in the background once old enough.

| Key | Type | Default | Effect |
|---|---|---|---|
| `webdav.url` | string | empty (off) | WebDAV server base URL. Empty means no uploads. |
| `webdav.username` / `webdav.password` | string | unset | HTTP Basic credentials. A password with no username is rejected and the request is sent unauthenticated. |
| `webdav.upload_timeout_secs` | integer | `300` | Timeout per upload request. |
| `webdav.poll_interval_secs` | integer | `3600` | How often the worker scans for eligible recordings. |
| `webdav.min_age_secs` | integer | `86400` | How old a recording must be before it is eligible (default one day). |
| `webdav.delete_on_upload` | boolean | `false` | Delete the local copy after a successful upload. |

## Web accounts

`web_users` is a list of accounts permitted to use the interface, managed through
the interface (not by hand) and stored in its own place rather than in the config
snapshot. Empty means no authentication, which is the default and keeps an update
from locking anyone out. There is no HTTPS on these devices, so accounts are a
second factor beyond the WiFi password, not a secure channel. See [Securing the
Web Interface](./web-authentication.md). **Fork, (not in template)**

## Where to next

- [Configuration](./configuration.md) — how to change these settings.
- [Compatibility With Upstream](./fork/compatibility.md) — which of these keys are
  fork-specific and what happens to them across versions.
