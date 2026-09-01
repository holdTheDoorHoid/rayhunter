# Device Notes

This is the landing page for the per-device details. If you are still choosing a
device, start at [Choosing a Device](../supported-devices.md), which covers
availability and which regions' frequencies each supports. This page is about
what is *device-specific* once you have one, chiefly the display, since that is
where devices differ most, and where several of this fork's features apply to
some devices and not others.

## At a glance

| Device | Display | Fork display features | Full page |
|---|---|---|---|
| Orbic RC400L (a.k.a. Kajeet RC400L) | Colour | All, including keep-screen-on | [Orbic](../orbic.md) |
| TP-Link M7350 | One-bit (pixel-art faces) | Colours and images do not apply | [TP-Link M7350](../tplink-m7350.md) |
| TP-Link M7310 | Colour | Colours, height, images; no keep-screen-on | [TP-Link M7310](../tplink-m7310.md) |
| TP-Link M7200 | Colour | Colours, height, images; no keep-screen-on | [TP-Link M7200](../tplink-m7200.md) |
| T-Mobile TMOHS1 | Colour | Colours, height, images; no keep-screen-on | [TMOHS1](../tmobile-tmohs1.md) |
| Wingtech CT2MHS01 | Colour | Colours, height, images; no keep-screen-on | [Wingtech CT2MHS01](../wingtech-ct2mhs01.md) |
| FY UZ801 | Colour | Colours, height, images; no keep-screen-on | [UZ801](../uz801.md) |
| Moxee Hotspot | Colour (Orbic-style) | Colours, height, images | [Moxee](../moxee.md) |
| PinePhone / PinePhone Pro | Headless (no screen) | Display settings do not apply | [PinePhone](../pinephone.md) |

A note on the TP-Link models: Rayhunter detects the display type from the
hardware at startup, a one-bit OLED gets the pixel-art faces, and anything else
falls back to the colour framebuffer path. The M7350 is the one-bit case; the
M7310 and M7200 use the colour path.

The **Orbic RC400L is the reference device**, the one Rayhunter was built and
tested on first, and this fork's primary development target. If you have a
choice and no regional reason to prefer another, it is the smoothest path, and it
is the device the [Quick Start](../quick-start.md) uses.

## What is device-specific

- **The display.** Colour devices draw Rayhunter's status line and can use the
  [colour, height, and image settings](../device-display.md). The TP-Link M7350
  has a one-bit display that shows pixel-art status faces instead and ignores
  colour and image settings entirely. The PinePhone is headless, it has no
  screen for Rayhunter to draw on, so the display settings do not apply.
- **Keep-screen-on.** This fork's [keep-screen-on](../device-display.md) setting
  is implemented on the Orbic. Other devices accept it in their config but ignore
  it, so it is harmless to leave set, it has no effect there.
- **Installation details.** Each device has its own installer command and quirks,
  covered on its page and summarised in [Installing from a
  Release](../installing-from-release.md).
- **Frequencies.** Which device works in your region depends on the radio bands
  it and your carriers use, check [Choosing a Device](../supported-devices.md)
  before buying.

## Adding a device

Rayhunter can theoretically run on a device with a Qualcomm modem that exposes a
diagnostic interface. If you want to add one, the [Porting to a New
Device](../porting.md) guide is the starting point.

## Where to next

- [Choosing a Device](../supported-devices.md), availability and regions.
- [The Device Screen](../device-display.md), the display settings the table
  above refers to.
- [Installation](../installation.md), getting Rayhunter onto your device.
