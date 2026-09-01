# The Device Screen

The device's own small screen is how Rayhunter tells you its state without a
computer nearby — running, paused, or warning, and how serious the warning is.
This page explains what the screen shows and the settings that change it. Most of
these settings are additions in this fork.

The screen is a separate surface from the [web dashboard](./web-interface.md).
Nothing here needs a browser.

## What the screen normally shows

By default Rayhunter draws a thin coloured line along the top of the device's
screen, over the device's own interface. The colour is the state:

- **Green** — running and recording. (Blue instead of green if you turn on
  colourblind mode.)
- **White** — paused.
- **Yellow, orange, or red** — a warning has been seen, at low, medium, or high
  severity, and stays until you reboot or start a new recording.

The severity is also shown by the line's **pattern** — dotted, dashed, or solid —
not only its colour. That is deliberate: the pattern is a colour-independent
channel, so the severity is legible even if you cannot distinguish the colours.
For that reason the pattern is not configurable.

## Display modes

The `ui_level` setting chooses how visible Rayhunter is on the screen:

- **0 — invisible.** No indication Rayhunter is running at all.
- **1 — subtle.** The thin status line described above. The default.
- **2 — demo.** A small orca animation, with the status line over it.
- **3 — EFF logo.** The logo, with the status line over it.
- **4 — high visibility.** The whole screen filled with the status colour. This
  is the same status line drawn at full height, not a separate feature.
- **5 — custom images.** Your own image per state (a fork addition; see below).
- **128 — trans flag.** A themed colour set.

In every mode except invisible, the status line (thin or full-screen) is what
carries the state, so you are never left guessing whether Rayhunter is running.

## Choosing your own colours (fork)

You can override the colour drawn for each state — paused, recording, and the
three warning levels — with the `display_colors` settings, each an `#rrggbb`
value. A malformed colour falls back to the built-in one rather than breaking the
display, so a typo cannot leave you with a blank screen. These apply only to
colour-capable displays.

## Status line height (fork)

The `status_bar_height` setting sets how tall the status line is drawn, in
pixels. It is clamped to the screen's height at draw time, so a value copied
between devices with different screens cannot produce a broken display. The
high-visibility mode ignores it, since it always fills the screen.

## Keeping the screen on (fork)

By default these devices blank their own screen on a timer to save power, which
can leave Rayhunter plainly running but dark. The `keep_screen_on` setting
controls this:

- **Never** — let the device blank as it normally would (the default).
- **Always** — hold the screen on whatever the power source.
- **Only while plugged in** — hold it on when external power is connected, and
  let it blank on battery.

The "only while plugged in" option exists because an always-on backlight is one
of the fastest ways to flatten a battery. This setting is implemented on the
Orbic; other devices accept it in their config but ignore it, so it is safe to
leave set.

## Reading the device's own screens (fork)

When Rayhunter fills or overlays the screen, it hides the device's own interface
underneath — including, on some devices, the screen showing the WiFi password.
So by default a **button press** shrinks Rayhunter to its thin status line for
about twenty seconds, letting you read the device's own screens, after which it
returns. This is the `pause_display_on_keypress` setting, on by default. A button
press is a good sign someone wants to see the device's own display, which is why
it triggers this.

## Custom images per state (fork)

With `ui_level` set to custom images, you can upload your own image for each
state, played when the device is in that state. An animated image plays as an
animation; a still image is shown as a still. Uploading an image is two steps:
the file is stored, and the setting for that state must point at it, or the
display will not use it. A state with no image falls back to drawing that state's
coloured status line.

## One-bit displays behave differently

Some devices, such as the TP-Link M7350, have a one-bit display that cannot show
colours. On those, Rayhunter draws small pixel-art status faces instead — a
running face, a warning face, a paused face — and ignores the colour and image
settings entirely. The state is still shown; it is shown in the only channel that
display has.

## Where to next

- [Configuration](./configuration.md) and [Configuration
  Reference](./configuration-reference.md) — where these settings live and their
  exact defaults.
- [Severity, and What It Means](./severity.md) — what the warning colours
  represent.
- [Device Notes](./devices/index.md) — which display each device has.
