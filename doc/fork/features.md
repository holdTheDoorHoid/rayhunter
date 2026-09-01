# What It Adds

This is the list of what this fork adds on top of upstream Rayhunter, grouped by
what you are trying to do rather than by which part of the code changed. Each
entry is one sentence and a link to the page with the full picture.

For why the fork exists at all, see [Why This Fork Exists](./index.md). For what
changes if you are switching from upstream, see [Differences You Will
Notice](./differences.md).

## Detecting more

- **Location-request detectors (LPP).** Warn when the network asks your device
  to measure and report its position, and, in more depth, when it asks for
  *continuous* tracking. See [Location Requested (LPP)](../detectors/lpp.md).
- **2G location detector (RRLP).** The same for the older 2G network your phone
  can fall back to, and the first thing in Rayhunter to read 2G signalling at
  all. See [Location Requested on 2G (RRLP)](../detectors/rrlp.md).
- **Timing-advance impersonation check.** Notices when one cell identity answers
  from a markedly different distance, which a real (stationary) tower does not do, silent on modems that do not report the measurement, the Orbic included. See
  [A Tower That Seems to Have Moved](../detectors/timing-advance.md).
- **2G and 3G traffic recorded, not dropped.** Legacy signalling is written into
  the capture for inspection instead of discarded, and no longer counted as a
  parse failure. See [Analyzing a Capture Yourself](../analyzing-a-capture.md).

## Seeing what is happening

- **The packet explorer.** Browse the actual messages in a recording, filter
  them, and jump to the ones that raised a warning. See [The Packet
  Explorer](../packet-explorer.md).
- **A cell-site panel.** The serving cell, its neighbours, the encryption in
  use, and detection health, shown live. See [The Web Interface, Panel by
  Panel](../web-interface.md).
- **This device's own identity.** Optionally shows your device's own IMSI, IMEI
  and temporary identity, off by default, because the interface has no
  password. See [The Web Interface, Panel by Panel](../web-interface.md) and
  [Securing the Web Interface](../web-authentication.md).
- **Warning counts by severity, and system health.** Warnings broken out per
  severity instead of one total, plus load, temperature, and recording headroom.
  See [The Web Interface, Panel by Panel](../web-interface.md).
- **Plain-language detector explanations, and a switch to hide them.** Each
  detector carries a plain description on the settings page, and the whole set of
  explanations can be hidden once you know them. See [The Web Interface, Panel by
  Panel](../web-interface.md).
- **A demo button.** Injects a clearly-labelled synthetic warning so you can see
  what one looks like on purpose. See [Your First Warning](../first-warning.md).

## Managing recordings

- **Names and notes.** Give a recording a display name and free-text notes,
  which also name the downloaded file. See [Recordings: Naming, Notes, and
  Rotation](../recordings.md).
- **Rotation.** Start a fresh recording automatically after a size or a length
  of time, so no single file grows too large to download. See
  [Recordings](../recordings.md).
- **Auto-delete of clean recordings.** When space runs low, remove analysed
  recordings that found nothing, never a named one. See
  [Recordings](../recordings.md).

## The device screen

- **Custom colours and status-line height.** Override the colour for each
  display state and how tall the status line is drawn. See [The Device
  Screen](../device-display.md).
- **Keep the screen on.** Stop the device blanking its screen on its own timer, never, always, or only while plugged in. See [The Device
  Screen](../device-display.md).
- **A button press to read the device's own screens.** Press a button and
  Rayhunter shrinks to its thin status line for twenty seconds, so the device's
  own display (including the WiFi password) can be read. See [The Device
  Screen](../device-display.md).
- **Custom images per state.** Play your own image for each display state. See
  [The Device Screen](../device-display.md).

## Securing and controlling access

- **Optional web accounts.** Put a password on the web interface, off by
  default, so an update never locks anyone out. There is no HTTPS on these
  devices, so this is a second factor beyond the WiFi password, not a secure
  channel. See [Securing the Web Interface](../web-authentication.md).
- **An optional web terminal.** Run a command on the device from the interface, enabled only at install time, never from the interface itself. See
  [Configuration](../configuration.md).

## Appearance and layout

- **Dark mode**, and a configuration page organised into sections rather than
  one long scroll. See [The Web Interface, Panel by Panel](../web-interface.md).

## A note on status

These features are at different stages of being offered to upstream, some
encouraged, some previously proposed and closed, some overlapping with another
contributor's work. [Differences You Will Notice](./differences.md) and
[Contributing Upstream](./upstreaming.md) record where each stands, so you know
which parts are likely to converge with upstream and which are not.

## Where to next

- [Differences You Will Notice](./differences.md), what a user of upstream
  notices on switching.
- [Compatibility With Upstream](./compatibility.md), whether you can move
  between the two versions.
