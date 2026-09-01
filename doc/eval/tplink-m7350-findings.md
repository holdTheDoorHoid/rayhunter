# TP-Link M7350 v8.0 — evaluation of this fork's `main`

Everything on `main` had been tested against the Orbic RC400L. This is the same
feature set exercised against a TP-Link M7350 hardware version 8.0, to find the
places where an Orbic assumption is baked in.

## The device under test

| | |
| --- | --- |
| Model | TP-Link M7350 **v8.0** (from `upnpd`'s `-mn M7350 -mv v8.0`) |
| SoC / kernel | Qualcomm mdm9607, Linux 3.18.20, armv7l, **single core** |
| Display | colour framebuffer, `/dev/fb0`, 128x128, 16bpp, **big-endian** RGB565 |
| RAM | 59.9 MB total, ~21 MB genuinely free |
| Rayhunter storage | `/data/rayhunter` is a **symlink to `/cache/rayhunter-data`** on `ubi0:cachefs` (67.6 MB) — *not* on `/data`, which is only 8.2 MB |
| SD card | none inserted; installed with `--skip-sdcard` |
| Root access | busybox telnet on port 23, **no authentication** |
| Was running | upstream Rayhunter 0.10.2 |
| Now running | this fork's `main` (0.12.2), cross-built `armv7-unknown-linux-musleabihf`, stripped to 10.6 MB |

## What the test could not cover

**The modem never attached to a network.** The SIM reads a home PLMN of
`311-480` (Verizon US) but the M7350 is a European-band unit (B1/B3/B7/B8/B20).
The device screen sits on "Connecting" with the signal icon crossed out, and
`/api/cell-info` shows `messages_seen: 0`. So nothing that depends on live
cellular traffic — cell info, neighbour cells, timing advance, SIM health
reaching a verdict, real analyser hits — could be exercised on this hardware.
Those paths were tested through the demo injection path instead, which uses the
same diag→IE→analyser chain.

---

## Findings

### 1. A full-screen display mode leaves its picture stuck on the screen for ever

**Severity: high (Invisible mode is the one it hurts most).**

Rayhunter does not own `/dev/fb0`, and this is documented for the Orbic, where
"the device's own interface keeps redrawing its parts of the screen". **On the
TP-Link that is not true.** `oledd` continuously repaints only the top status
icons (battery, wifi); the body of the screen is repainted on events, not on a
timer.

The consequence: after any UI level that covers the screen (Demo, EFF logo,
High Visibility, Custom GIF), switching to Subtle or Invisible leaves the last
full-screen picture on the display permanently.

Measured: fill the screen from High Visibility mode (16384/16384 pixels green),
then switch to `ui_level = 0` (Invisible) and sample for five minutes:

```
 ~10s after switching to Invisible: 16160/16384 still green
 ~40s                             : 16160/16384
 ...
~310s                             : 16160/16384
```

**98.6% of the screen is still Rayhunter's green five minutes after Rayhunter
was told to be invisible.** The only pixels that come back are the 224 belonging
to the battery icon, which `oledd` repaints on its own timer.

It clears when something makes the device redraw its own screen — a button
press, or a state change it cares about — not on any schedule. Restarting the
vendor daemon (`/etc/init.d/start_oledd restart`) also restores it, and is the
obvious basis for a fix. `cmd_oled -c` followed by `-o` does **not**: measured
0 of 16384 pixels changed, because that controls panel power and backlight
only, not content.

This is worst for Invisible mode, whose entire purpose is that there is no
indicator Rayhunter is running. Instead the device shows a full-screen orca.

It also degrades Subtle mode (level 1): the thin status line is drawn correctly
over a frozen image of whatever was last displayed, instead of over the device's
own interface.

The same applies when the daemon **stops**: the display loop breaks out on the
shutdown token without restoring anything, so stopping or uninstalling Rayhunter
from a full-screen mode leaves its last frame on the device.

### 2. In Demo mode a warning is on screen for a fraction of every GIF cycle

**Severity: medium. Not TP-Link specific — this is shared code and the Orbic
runs it too — but it showed up here and it is worth a decision.**

`draw_gif_interruptible` breaks out of a playthrough as soon as a state update
arrives, and `CLAUDE.md` records that interrupt as 0.19 s on an Orbic. What it
does not record is what happens next. The `UiLevel::Demo` arm discards the
`interrupted` return, so the loop:

1. breaks out of the GIF, and draws the status line using the state it read at
   the *top* of that pass — still the old one;
2. comes round again, reads the new warning state, and calls
   `draw_gif_interruptible` again — the channel is empty now, so it plays the
   **whole** GIF, about 16 seconds;
3. finally draws the red line — and immediately replays the GIF from frame 0
   over the top of it.

So the red line exists for roughly one loop iteration per GIF cycle.

Measured on the TP-Link with a high-severity demo warning standing and
`ui_level = 2`, eight independent low-load framebuffer captures:

```
sample 1: red=0    sample 5: red=0
sample 2: red=0    sample 6: red=0
sample 3: red=100  sample 7: red=0
sample 4: red=0    sample 8: red=0
```

(`red` = how many of the first 100 pixels of row 0 are the warning red.)

One capture in eight. A 1 Hz sampler running for 28 seconds across a trigger
never caught it at all.

Two earlier measurements of mine were wrong and are worth recording so nobody
repeats them: requiring all 128 pixels of the row to match misses the line
entirely, because the device's own battery icon owns the last ~14; and a
free-running `dd`/`od` sampler saturates the single core, starves the display
task, and leaves the red line standing in 1487 of 1500 samples — the opposite
conclusion, from load the test itself created.

In every other level the line is stable, because `draw_img` and the still-image
path are fast and the line is the last thing drawn before the sleep. Demo is the
only mode where the picture takes the whole cycle.

Whether this is a bug depends on what Demo mode is for. If it is only the fun
one, a warning that blinks once per 16 seconds is a defensible trade. If it is a
mode people actually leave running, it is a detector whose alert is easy to
miss. Cheapest fix: use the `interrupted` return that is already there, and skip
the GIF for one pass when it is true.

### 3. The screen says "recording" when recording was refused for lack of space

**Severity: high. The display asserts a detector is running when it is not.**

Set `min_space_to_start_recording_mb` above the free space and restart. The
daemon correctly refuses:

```
$ curl -X POST /api/start-recording
Insufficient disk space: 55MB available, 999MB required
HTTP 507
$ curl /api/qmdl-manifest    ->  "current_entry": null
```

The device's status line stays **solid green — "recording"**. Measured: row 0
was `rgb(0,252,0)` across all 128 pixels while `current_entry` was null.

The cause is a missing state transition, not a display fault.
`DiagTask::start()` returns `Err(InsufficientDiskSpace)` at the disk check,
which is *before* the `ui_update_sender.send(DisplayState::Recording)` at the
end of the function. The display task initialises `let mut state =
DisplayState::Recording;` and only ever changes it when a message arrives. No
message is ever sent, so it sits on its initial value, which happens to be the
one that means everything is fine.

The mid-recording path is **correct** by contrast: `DiskSpaceCheck::Critical`
calls `self.stop(..)`, which does send `Paused`, and the line goes white. So
this is specifically "never started", not "stopped".

Any early return from `start()` has the same effect — `new_entry()` failing on
I/O would also leave a green line over a device that is recording nothing.

This is shared code, so an Orbic behaves the same way. It matters more here:
the TP-Link keeps recordings on a 67 MB cache partition, and
`doc/tplink-m7350.md` says the device "needs a FAT-formatted SD card to
function for more than a few hours". A unit installed with `--skip-sdcard` —
which is how the device under test was set up — is *expected* to run out of
room, and this is what it will show when it does.

The fix is a line: send `Paused` on the error paths out of `start()`, or make
the display's initial state something other than `Recording`.

### 4. `radio_temp_c` reads 125 °C on this hardware

**Severity: medium (visible, alarming, wrong).**

`/api/system-stats` reports `radio_temp_c: 125.0`. The device has two power
amplifier sensors:

```
thermal_zone6 type=pa_therm0 temp=29
thermal_zone7 type=pa_therm1 temp=125
```

`pa_therm1` is unpopulated on this board and reports the Qualcomm tsens
sentinel value of 125. `read_temperatures` in `daemon/src/stats.rs` takes the
**maximum** across everything named `pa_therm`, and its sanity window is
`-40..=150`, so 125 passes and wins. The web UI then prints "125°C Radio".

The Orbic evidently has a single PA sensor, which is why taking the max was
never wrong there.

The same function also mis-classifies `thermal_zone0`, whose type is `battery`
and which reports **deci**-degrees (`2800` = 28.0 °C). The scaling rule divides
anything above 200 by 1000, giving 2.8 °C, and the name does not contain
`pa_therm` so it is filed as a *processor* sensor. It is hidden today only
because `max()` picks the warmer tsens cores.

### 5. "Keep the screen on" is offered on a device where it does nothing

**Severity: low-medium (a control that silently does nothing).**

`keep_screen_on` is implemented only in `daemon/src/display/orbic.rs`;
`display/tplink.rs` never calls it. That is deliberate and documented in
`config.rs` ("other devices ignore it rather than failing").

But `ConfigForm.svelte` renders the "Keep the screen on" select
**unconditionally**, with no device check. On a TP-Link the setting saves, the
daemon restarts, and nothing whatsoever happens — and nothing is logged either.

The TP-Link does have the equivalent control: `/usr/bin/cmd_oled -o` ("open
oled panel and backlight") and `-c` to close, which is what the stock
`kill_oled` init script uses. So this is implementable here rather than merely
hideable.

### 6. Installing Rayhunter leaves an unauthenticated root telnet open, permanently

**Severity: high, and it undercuts a feature this fork added.**

On non-v3 hardware the installer persists itself through the TP-Link's
port-trigger table (`installer/src/tplink.rs`), registering two entries whose
`triggerPort` fields are command injections:

```
applicationName: "rayhunter-daemon", triggerPort: "$(/etc/init.d/rayhunter_daemon start &)"
applicationName: "rayhunter-root",   triggerPort: "$(busybox telnetd -l /bin/sh &)"
```

That is how the daemon starts at boot — there is no `rc5.d` symlink for it. The
side effect is that **`busybox telnetd -l /bin/sh` runs on port 23 on every
boot, giving a root shell with no password to anyone on the hotspot.** Confirmed
live: `telnet 192.168.0.1` returns `uid=0(root)` after 11 hours of uptime.

This matters specifically because of two features on this branch:

* **web-authentication** — the web UI password is bypassed entirely by telnet.
* **web-terminal** — it is gated behind `terminal_enabled` *and* a web account,
  reasoning that "the root shell must never be reachable without a web
  password". On this device the root shell is already reachable without one.

The hardening is still correct, but on TP-Link it is defending a door next to an
open window, and the docs should say so.

The entries live in `/data/config/port_forwarding` as `port_trigger1` and
`port_trigger2`, with every value AES-encrypted by the firmware — which is why
grepping the filesystem for `telnetd` or `rayhunter` finds nothing. Two
consequences follow:

* They survive reboots. Confirmed: after a `reboot`, telnet and the daemon were
  both back within 20 seconds.
* They are **visible and deletable in the TP-Link's own admin UI**, under port
  triggering, as two rules named `rayhunter-daemon` and `rayhunter-root`. A user
  tidying up what look like stray firewall rules would stop Rayhunter starting
  at boot, with nothing to explain why.

### 7. `firmware-devel` builds are too large to deploy comfortably

**Severity: low (developer-facing).**

The unstripped `firmware-devel` daemon is **20.8 MB**. Rayhunter's partition on
this device has 59.7 MB free, so an unstripped dev build takes a third of it.
`llvm-strip --strip-all` brings it to 10.6 MB. Worth a line in `CLAUDE.md`
alongside the Orbic deploy notes.

---

## What works correctly on the TP-Link

Verified by measurement, not by looking.

* **Binding the web interface to the hotspot and loopback only** (`web-bind-lan-only`).
  Binds `127.0.0.1:8080` and `192.168.0.1:8080`. Correctly *excludes* the
  `169.254.3.1` rndis address, which is link-local rather than RFC1918.
* **Big-endian RGB565 conversion.** The TP-Link driver writes BE where the
  Orbic writes LE; decoding readbacks as BE gives exact colours.
* **All five status-line states**, colours and patterns:
  Recording solid green, Paused solid white, warning-low yellow dotted (1 on /
  3 off), warning-medium orange dashed (4 on / 4 off), warning-high solid red.
* **`status_bar_height`.** Set to 12, measured exactly 12 rows.
* **`display_colors` overrides.** `#0000ff` produced rgb(0,0,248), the correct
  RGB565 rounding.
* **UI level 3 (EFF logo)** and **level 4 (High Visibility)** — only the
  device's own battery icon pokes through, which is the known shared-framebuffer
  behaviour.
* **UI level 2 (demo orca GIF)** animates, and costs **~15.5% of the single
  core** (155 jiffies in 10 s).
* **UI level 5 (custom image).** A PNG uploaded as `recording.gif` was detected
  from its magic bytes and drawn as a still. The status line is suppressed in
  this mode by design.
* **Config upgrade in place** from 0.10.2's `config.toml` to 0.12.2 — every new
  field defaulted correctly.
* **Reading the SIM's home PLMN** (`311-480`) over the TP-Link's AT interface.
* **Recording** starts and grows on the cache partition.

### 8. The USB network interface gets a new MAC on every boot

**Severity: low, but it breaks test tooling and is worth knowing.**

The TP-Link's RNDIS interface comes up with a different MAC each boot, so the
host's interface name and DHCP lease both change:

```
before reboot:  enx1a7e9ebb13f0   192.168.0.174
after reboot:   enx324015a791e2   192.168.0.141
```

The device is always `192.168.0.1`, so anything addressing the device is fine.
Anything that hardcodes the **host's** address — a `nc` callback, a reverse
tunnel, a script that tells the device where to send something — silently breaks
after a reboot. Look the address up instead:

```bash
ip -4 -br addr | awk '$3 ~ /^192\.168\.0\./ {split($3,a,"/"); print a[1]; exit}'
```

---

## Verified after a cold reboot

The device was rebooted with this fork's daemon installed, and came back in
about 20 seconds:

* **Rayhunter started on its own** (pid 1244, md5 of the binary unchanged), via
  the port-trigger entry — there is no `rc5.d` symlink for it.
* **The web interface bound `127.0.0.1` and `192.168.0.1` only** at cold boot:

  ```
  serving the web interface on port 8080, addresses [127.0.0.1, 192.168.0.1]
  ```

  No `0.0.0.0` fallback, so `bridge0` has its address before the daemon binds on
  this hardware, exactly as on the Orbic. `netstat` confirms two listeners and no
  wildcard.
* **Recording restarted** with 53 MB free.
* **The status line came back** green over the device's own interface.
* **The battery read cleanly** (86%, plugged in) with no parse error in the log —
  the `Failed to get battery status` warning seen under 0.10.2 at boot did not
  recur.

## Web interface, checked in a browser

* The **clock mismatch prompt** works and is the right call on this device: with
  no WAN there is no NTP, and the device clock was three years slow
  (2023-09-11 against a real 2026-09-01). It shows both clocks and offers
  "Sync Clock".
* The **Configuration modal** renders correctly — tabbed (Display, Detection,
  Recordings, Notifications, Network), with live colour/pattern preview swatches
  that match what the device actually draws.
* **Severity counts** in the history are right: the recording carrying the demo
  warnings shows "5 High / 2 Medium / 4 Low / 3 Info".
* **Recording names** show above the ID, as designed. Note `sanitize_display_name`
  turns spaces into underscores, so "TP-Link eval run" is stored and displayed as
  `TP-Link_eval_run`. Deliberate (the name reaches a filename), but the field
  gives no hint that it will happen.
* `/config`, `/logs` and `/terminal` return 404 — **not a bug**. The interface is
  a single page and those are panels, not routes; the URL never leaves
  `/index.html`.

## API surface exercised

| Area | Result |
| --- | --- |
| `start`/`stop`/`delete` recording | works; deleting an unknown name gives 400 with a clear message |
| annotate (name + notes) | works |
| `pcap` / `qmdl` / `zip` download | all 200, valid pcapng and zip |
| analysis + report | 11 analysers registered, runs in seconds |
| demo warning injection | fires the full chain: high-severity 2G downgrade plus LPP request and tracking events, and turns the device's line red |
| packet explorer | decodes LTE RRC SIB1 with PCI, EARFCN, SFN |
| `min_space_to_start_recording_mb` guard | correctly refuses with HTTP 507 — but see finding 3 |
| time-based rotation | works; `stop_reason` reads "reached the 1 minute limit" |
| web accounts | HTTP Basic; everything 401s without credentials once an account exists, including `/index.html` |
| web terminal | root shell, correct exit codes, stdout/stderr split, 15 s timeout, output capped at ~256 KB without exhausting RAM |
| terminal gating | cannot be enabled from the interface — `set_config` overwrites `terminal_enabled` from the startup snapshot, by design |
| GPS API mode | POST then GET round-trips coordinates; correctly 403/404 when `gps_mode` does not match |
| notifications | correct errors both with no URL (400) and with no route to the internet (500) |
| config upgrade 0.10.2 → 0.12.2 | every new field defaulted correctly in place |

## Not exercised, and why

* **Anything needing a live network**: cell info, neighbour cells, timing
  advance, a SIM-health verdict, real analyser hits, IMSI/IMEI display
  (`show_subscriber_identity` reads identities out of NAS messages, and there
  were none). The M7350 is a European-band unit and the SIM's home PLMN is
  Verizon US.
* **`auto_delete_clean_recordings`**: exercising it means deleting recordings
  that are not mine. The code path is plain filesystem work with no
  device-specific branch, but it has not been run on this hardware.
* **Button handling** and **battery discharge**: both need somebody at the
  device. See the open questions below.

## Open questions that need the physical device

1. **Press the power button once** while Rayhunter is in a full-screen mode
   (Demo or High Visibility). Two things to watch: whether the screen steps
   aside to the thin line (`pause_display_on_keypress`), and whether the
   device's own interface actually **repaints** underneath — which is what
   decides whether finding 1 also breaks the "let me read my wifi password"
   feature, or only affects mode changes.
2. **Double-tap the power button** with `key_input_mode = 1`, to confirm it
   starts a new recording. Worth doing because `key_input.rs` reads
   `/dev/input/event0` in **32-byte** blocks while this kernel's
   `struct input_event` is **16 bytes** (32-bit ARM: two 4-byte timeval fields,
   plus type, code, value). The unit tests are named `..._m7350_v5` and their
   data is a 16-byte event zero-padded to 32. It works today only because key
   events arrive in exact EV_KEY/EV_SYN pairs, so one 32-byte read happens to
   span one pair and byte 12 lands on the value. Any device that emits an odd
   number of events — an EV_MSC scancode before EV_KEY, which `gpio-keys`
   commonly does — desynchronises the stream permanently. There is a second
   input device here (`event1`, `gpio-keys`) that nothing reads.
3. **Unplug the USB cable** for a few minutes, to check the battery reading
   tracks discharge and that a low-battery notification would fire. `uci get
   battery.battery_mgr.is_charging` returned 1 the whole time it was plugged in,
   while the level fell from 96% to 86% under test load — worth knowing whether
   that flag means "charging" or just "USB present".
