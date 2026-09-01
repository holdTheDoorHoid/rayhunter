# Power and Battery Life

What Rayhunter costs to run, measured rather than guessed, and which settings
actually change it.

## Rayhunter is not the expensive part

Measured on two devices while recording, sampling the daemon's CPU time over a
minute of steady running:

| Device | CPU while recording | Memory |
|---|---|---|
| Orbic RC400L | about **1%** of one core | 4.7 MB |
| TP-Link M7350 | about **6.5%** of one core | 4.5 MB |

The difference between the two is the hardware and how much the modem has to
say, not anything configurable. Switching the TP-Link between the thin status
line and the full screen display made **no measurable difference** to its CPU
use, so choosing a quieter display mode is not a way to save power.

Rayhunter's logging is not a constant drain either. It writes in bursts when the
modem produces something it cannot parse, not continuously; the log did not grow
at all over a minute of ordinary recording.

## What does use the battery

On a hotspot the real draws are the cellular radio, the screen, and the WiFi
access point. Rayhunter cannot do anything about the first, and should not: the
radio is the thing being watched.

The two you can control:

- **The screen.** Rayhunter can hold the backlight on, which is a real cost.
  See `keep_screen_on` in the [Configuration Reference](./configuration-reference.md).
- **The WiFi access point.** A device running as a sensor does not need to be
  broadcasting a network. See "Switching WiFi off from the buttons" in
  [Configuration](./configuration.md).

## The shortcut

The Display tab of the configuration page has a **Set up for longest battery
life** button. It turns the device display off, stops Rayhunter holding the
screen awake, stops the periodic update check, and allows switching WiFi off
from the buttons.

It changes the settings rather than saving them, so you can see exactly what it
did before pressing save.

**It deliberately leaves the detectors alone.** Detection is cheap, and turning
it off to save power would defeat the point of carrying the device.

## What is not measured here

Actual battery endurance in hours. That needs a current measurement on the
battery rail over a long run, not a CPU sample, and no such measurement has been
made. The numbers above say what Rayhunter costs the processor; they do not say
how long a given device will last.
