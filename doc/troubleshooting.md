# Troubleshooting

Symptoms, their likely causes, and what to do. Error messages are quoted
verbatim so that searching this page for the text you see on screen finds the
right entry.

If your problem is not here, the [FAQ](./faq.md) and [Support, Feedback, and
Community](./support-feedback-community.md) are the next places to look.

## Installing

### "No Orbic device found" (macOS)

**Cause:** macOS is blocking the USB connection to the device, usually because of
the "Allow accessories to connect" security setting.

**Fix:** open System Settings → Privacy & Security, set "Allow accessories to
connect" to "Always" temporarily, and run the installer again. Change it back to
a more secure setting when you are done.

### The installer says the device is "being used by another program"

**Cause:** an `adb` server on your computer is holding the device, so the
installer cannot get exclusive access to it.

**Fix:** stop the adb server (`adb kill-server`) and run the installer again. You
can start it again afterward if you need it.

### The installer fails partway, or cannot connect at all

**Cause:** most often a flaky USB connection.

**Fix:** try a different USB cable, connect the device directly to a USB port
rather than through a hub, and try a different port. A faulty cable is the single
most common cause of an installer that fails inconsistently. If you are
installing over the device's WiFi instead, confirm you can reach the device's
admin page in a browser first.

### No green line on the device after installing

**Cause:** the device may not have finished starting, or Rayhunter did not start.

**Fix:** give it up to two minutes after the reboot. If there is still no green
line, power the device fully off and on again (a full power cycle, not only a
reboot). If it still does not appear, re-run the installer.

## Reaching the web interface

### The browser warns the connection is "not secure" / "not private"

**Cause:** the device has no HTTPS. This is expected, not a fault.

**Fix:** continue past the warning — look for "Advanced," "Show details," or
"visit this website" if there is no obvious button. See [Using
Rayhunter](./using-rayhunter.md).

### "Insecure download blocked" when downloading a recording

**Cause:** the same absence of HTTPS, applied to the download.

**Fix:** allow the download. It is safe; the warning is only about the lack of
HTTPS.

### The page is stuck on "Loading..." forever

There are two common causes, so check them in order:

**Cause 1 — the browser tab is hidden or collapsed.** Rayhunter's page pauses its
updates when the tab is not visible, so a backgrounded or collapsed pane can sit
at "Loading..." with everything actually healthy.

**Fix:** bring the tab fully to the foreground and give it a few seconds.

**Cause 2 — an older version with GPS disabled.** In older versions, a page could
hang on "Loading..." when GPS was off (the normal setting). This fork fixes that.

**Fix:** if you see this, updating resolves it. See [Updating
Rayhunter](./updating-rayhunter.md).

### An error says the thing answering is not your Rayhunter device

**Cause:** your request reached something other than the Rayhunter daemon — a
phone on mobile data instead of the device's WiFi, a VPN, a captive portal, or a
different device answering at that address. The page it got back is not
Rayhunter's data. (This clear message is a fork fix; older versions showed a
confusing parsing error instead.)

**Fix:** make sure you are on the device's own WiFi (or USB connection), turn off
any VPN, and confirm you can reach the device's admin page. If several devices
answer at `192.168.1.1`, make sure you are connected to the right one.

## While running

### The dashboard is unresponsive for about a minute after saving settings

**Cause:** saving the configuration restarts the daemon. This is normal.

**Fix:** wait about a minute; the interface returns on its own. Do not save
repeatedly during the wait.

### The Cell Site panel shows the same reading and never changes

**Cause:** the radio measurements behind that panel only arrive while the modem
is active — attaching, moving, recovering. An idle, stable device stops producing
them, so the panel legitimately holds its last reading.

**Fix:** none needed; this is the modem being quiet, not a fault. The panel
updates again when the device next does radio work. A full power cycle brings the
measurements back if you want to confirm.

### Warnings are flooding in on every tower

**Cause:** the test analyzer is on. It fires on every tower by design, to prove
the pipeline works.

**Fix:** turn off the "Alert on every tower, for testing" detector in settings.
Leave it off while actually hunting. See the [Detector
Reference](./detectors/index.md).

### A recording contains a warning you made with the demo

**Cause:** the demo feature was on and injected a labelled practice warning into
the recording.

**Fix:** that recording is not evidence — set it aside and start a fresh one, and
turn the demo off in settings. Never send a demo-tainted recording to EFF or
present it as real. See [Your First Warning](./first-warning.md).

## Where to next

- [Frequently Asked Questions](./faq.md).
- [Support, Feedback, and Community](./support-feedback-community.md).
- [Reading Warnings Without Panicking](./concepts/interpreting-warnings.md) — if
  the "problem" is a real warning and you are deciding what it means.
