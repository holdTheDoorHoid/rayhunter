# The Web Interface, Panel by Panel

Rayhunter's dashboard is where you start and stop recordings, review what it
found, and change its settings. This page walks through each panel and what it
means. It is a reference to keep open while you learn the interface, not
something to read end to end.

To reach the dashboard, connect to the device and open its address with `:8080`
on the end, `http://192.168.1.1:8080` on the Orbic. Your browser will warn that
the connection is not secure, because the device has no HTTPS; that warning is
expected and safe to pass. [Using Rayhunter](./using-rayhunter.md) covers the
connection options in full.

Several panels described here are additions in this fork; each is marked. Which
panels you see also depends on your settings, the demo and terminal panels
appear only when switched on.

## Recording controls and the current recording

At the top, the recording controls let you start and stop recording. While a
recording runs, this area shows its status and a running count of any warnings,
**broken out by severity** rather than as a single total (a fork addition), so
you can tell one high-severity finding from a dozen low ones at a glance.

Starting a new recording resets the device's status line, which is how you clear
a warning colour after you have reviewed it.

## The recording history

Below the controls is the list of past recordings. Each entry shows when it was
made, its size, and its warning counts, and lets you open, download, or delete
it. In this fork you can also give a recording a **display name and notes**, and
that name is used for the downloaded file, see [Recordings: Naming, Notes, and
Rotation](./recordings.md).

Downloading a recording may prompt an "insecure download" warning, again because
there is no HTTPS; it is safe to allow. What a downloaded recording contains, and
how to redact it before sharing, is covered in [Sharing What You
Find](./sharing-findings.md).

## Opening a recording: the analysis and the packet explorer

Opening a recording shows its analysis, the warnings, with their severity and
which detector raised each. From here you reach the **packet explorer** (a fork
addition), which lists the individual messages in the recording, lets you filter
them, marks the ones that raised a warning, and lets you jump straight to them.
The packet explorer has its own page: [The Packet Explorer](./packet-explorer.md).

This is the heart of reviewing a finding: the analysis tells you *what* was
flagged, and the packet explorer lets you see the actual messages behind it, so
you or someone you trust can check the finding rather than take it on faith.

## The Cell Site panel

This panel (a fork addition) shows what your device can see about the network
around it, live:

- **The serving cell**, the one tower your device is actually connected to and
  can identify, with its cell identity and radio details.
- **Neighbouring cells**, the other cells your device has seen this run, listed
  under "cells seen this run."
- **Encryption in use**, the ciphers in effect at both the radio layer and the
  core-network layer, shown separately (each can be "not seen" until observed).
  This is where a null cipher would show as the absence of encryption.
- **Whether the SIM is working**, a plain verdict so a dead, unactivated, or
  unseated SIM is not mistaken for a quiet night. It reads "SIM is working" once
  the device is talking to a network, "Checking the SIM" while it is still
  registering, and "SIM may not be working" when the device can see towers but
  has not managed to register with any of them. The distinction it draws is
  between hearing a network and talking to one: a SIM-less modem still decodes
  the towers around it, so seeing cells proves only that the radio works.
- **This device's own identity**, its IMSI, IMEI, and temporary identity, but
  **only if you have turned that on**, under **Configuration → Security**. It
  is off by default: the IMSI is exactly what an IMSI catcher wants, and
  anything this interface shows can be read from every paired device. See
  [Securing the Web Interface](./web-authentication.md) before enabling it.

A caution worth carrying: the radio measurements behind this panel arrive only
when the modem is active, attaching, moving, recovering. An idle, stable device
stops producing them, so the panel can legitimately show the same reading for a
long stretch. That is the modem being quiet, not the panel being stuck.

## System health

The system panel (a fork addition) shows the device's load, uptime, temperature,
and how much recording space is left, using measured processor usage rather than
a rough average. It is useful for spotting a device running hot, low on space, or
struggling, any of which can affect recording.

## Settings

The configuration form is where every setting lives, organised into sections (a
fork layout change) rather than one long scroll: Display, Detection, Recordings,
Notifications, Network, and Security. The last holds who may use the interface,
the owner passphrase, the unit's certificate, USB debugging, and whether the
device discloses its own identity. Each detector carries a
plain-language description here, so you can decide whether to switch one off
without needing to know what a name like "NAS null cipher" means, and there is a
switch to hide those explanations once you know them.

For what every setting does, see [Configuration](./configuration.md) for the
how-to and [Configuration Reference](./configuration-reference.md) for the
exhaustive list. The detector toggles are explained in the [Detector
Reference](./detectors/index.md).

## Notices and alerts

A few things appear only when relevant:

- **Update notice**, if update checking is on and a new release exists.
- **Clock-drift alert**, if the device's clock has drifted from your browser's,
  with the option to sync, depending on your clock-sync setting.
- **Action errors**, if something you asked for failed, including the specific
  message that appears when the thing answering is not actually your Rayhunter
  device (a fork fix that tells you *that* rather than showing a confusing parse
  error).

## Panels that appear only when enabled

- **The demo panel**, an amber, dashed-outline box for injecting a
  clearly-labelled practice warning, shown only when demo mode is on. See [Your
  First Warning](./first-warning.md).
- **The terminal**, a box for running a single command on the device, shown only
  when the terminal was enabled at install time. It runs as root, so it is
  deliberately not something the interface can switch on by itself.

## The device screen is a separate surface

Everything above is the web dashboard. The device's own small screen, the
status line, its colours, and how it behaves, is configured separately and
described in [The Device Screen](./device-display.md).

## Where to next

- [The Packet Explorer](./packet-explorer.md), reading the messages behind a
  warning.
- [Recordings: Naming, Notes, and Rotation](./recordings.md), managing captures.
- [Securing the Web Interface](./web-authentication.md), before you expose
  identity data or rely on the interface being private.
