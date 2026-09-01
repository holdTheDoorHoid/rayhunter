# A Tower That Seems to Have Moved

Watches whether a cell keeps answering from the same distance, because real
towers do not move.

This detector is an **addition in this fork** and is not present upstream. It
also has an unusual and important limitation: **it is silent on the Orbic**, the
reference device, see below.

## What you would see

A Low warning saying a cell reported a different distance than it did before, that a tower you had been talking to now appears to be roughly a kilometre or
more from where it was. The warning states the change in plain terms and says,
in the same breath, that if your device moved, that explains it. On a device
that does not report the underlying measurement, the Orbic among them, you
would see nothing from this detector at all, ever.

## Why it matters

A [cell-site simulator](../concepts/cell-site-simulators.md) attracts phones by
copying a real cell's identifiers, so on paper it looks like a tower your phone
already trusts. The one thing it cannot copy is where that real tower physically
stands. So if a single cell identity answers from two clearly different
distances, and your device did not move, that is a strong sign that two
different transmitters are using the same name, which is what impersonating a
tower looks like from the inside.

This is a different kind of evidence from the other detectors. Most of them read
the *content* of a message for a suspicious pattern. This one uses a physical
fact, a tower stays put, to notice a spatial inconsistency, in the spirit of
the detection-by-artifact and learn-normal-then-flag-deviations approaches in
the research ([Dabrowski et al.](../references.md);
[SeaGlass](../references.md)).

## When it fires harmlessly

The honest headline is a strong one, so read this section before acting on a
warning:

- **You moved.** This is the big one, and the detector says so in its own text.
  Carry the device any real distance and the towers around it genuinely change
  distance, and this fires honestly. On a device someone is walking or driving
  around, a warning here is *expected*, not suspicious. The detector is Low
  severity precisely because movement is such a common innocent cause.
- **Signal that bounces.** The distance is measured along the signal path, not
  the map, so reflections off buildings and terrain can shift the figure without
  the tower or the device moving. The detector's threshold is set wide, about
  1.2 km, to stay above this kind of variation, but it is a reason not to read a
  single borderline warning as meaningful.

The practical reading: **this detector is informative mainly when the device was
stationary.** On a device sitting on a desk, a warning is worth attention; on one
being carried, it is most likely you. That context is yours to supply, the
detector cannot know whether you moved.

This detector has **not** been confirmed against a real cell-site simulator; no
recording of a genuine two-transmitter distance jump has been run through it.
Its logic is covered by unit tests, and the crucial fact that the Orbic reports
no usable measurement was checked against real attaches (see Precise behavior).
[How We Validate Detectors](./validation.md) explains what that level of
evidence is worth.

## How it works

Every time your device begins talking to a tower, the tower sends a small
correction called *timing advance*, telling the device how far ahead to send so
its transmission lands in the right slot. That correction is proportional to the
round-trip distance, very roughly 78 metres per step, and it is the only
distance-to-the-tower figure a device gets for free.

The detector attributes each timing-advance value to the cell the device is
currently camped on (learned from the tower's identity broadcast). For each
cell, it establishes a baseline from the first couple of measurements, then
watches for a later measurement that differs from the baseline by more than its
threshold. When one appears, it warns, and then follows the cell to its new
distance, so a device that genuinely moved reports the jump once, not forever.

Two safeguards keep it from crying wolf, both worth knowing:

- **It waits for a baseline** (more than one sample) before judging a cell, so a
  single noisy measurement does not flag anything.
- **It requires having seen a real measurement at all.** Some modems report a
  timing advance of zero for everything. Treating that as "always the same
  distance" would make the detector look alive while being unable to ever fire,
  so an all-zero history is treated as *no data*, and the detector stays silent.

## Precise behavior

- **Code identifier:** `timing_advance`.
- **Source:** `lib/src/analysis/timing_advance.rs`; analyzer version 1.
- **Severity:** Low, only. Movement is too common a cause to justify anything
  higher.
- **What it reads:** the timing advance in an LTE MAC *random access response*,
  attributed to the current cell (from the SIB1 cell identity). Reading that MAC
  message at all is a fork addition, it is the first LTE MAC message to reach an
  analyzer.
- **Threshold:** a change of about 1.2 km (16 timing-advance steps at roughly 78
  metres each) from the cell's established baseline.
- **Deduplication:** per cell. A baseline is set from the first two samples; a
  warning updates the baseline to the new distance, so a genuine move fires once
  rather than repeatedly. At most 32 cells are remembered at a time.
- **What it deliberately ignores:** changes below the threshold; any cell before
  its identity is known; and, critically, modems that only ever report zero.
  **The Orbic RC400L is one of these:** it returns zero timing advance on every
  random access, checked against three real attaches and cross-checked with
  SCAT's parser. On the Orbic, and any modem like it, this detector never fires.
  Since the Orbic is the reference device, that means the detector is effectively
  dormant on the most common hardware, a limitation stated plainly here rather
  than buried.
- **Validation:** unit tests cover the baseline, threshold, per-cell tracking,
  the move-follows behavior, and the all-zero guard. The Orbic's all-zero
  behavior was verified against real device traffic. The detector has **not**
  been run against a real cell-site simulator producing a genuine distance jump.

## Configuration

Enabled by default. The key is `timing_advance` under `[analyzers]`, or the
"A tower that seems to have moved" toggle on the settings page. It costs nothing
to leave on: on a modem that does not report timing advance (such as the Orbic)
it is silent, and on one that does, its warnings are Low and self-explaining.
[Configuration](../configuration.md) covers applying analyzer toggles.

## Sources

- **The detection principle.** Dabrowski et al., "IMSI-Catch Me If You Can"
  (detection by artifact), and the SeaGlass approach of flagging deviations from
  a network's normal behavior, both in [Sources and Further
  Reading](../references.md).
- **The measurement.** 3GPP TS 36.321 (E-UTRA MAC): the random access response
  and its timing advance command, which is the value this detector reads.
- **In this book.** [What a Cell-Site Simulator
  Is](../concepts/cell-site-simulators.md) for why a fake tower shares a real
  cell's identity but not its location, and [How We Validate
  Detectors](./validation.md) for the evidence standard.
