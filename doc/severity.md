# Severity, and What It Means

When Rayhunter raises a warning, it attaches a severity: Low, Medium, or High.
That label is the tool's own estimate of how much the finding is worth your
attention — a first sort, so that a fleeting oddity and a serious signal do
not arrive looking identical. This page explains what each level is meant to
convey, and one quiet design decision that changes how you should read a whole
report.

Severity is a starting point for judgement, not a substitute for it. How to
turn a severity into an actual decision is the job of [Reading Warnings Without
Panicking](./concepts/interpreting-warnings.md); this page defines what the
levels are.

## What you would see

In the web interface and on the device, warnings are marked by level, and the
device's status line changes colour to the most serious thing in the current
recording. There are four levels in the underlying data, and the difference
between the first and the rest is the important part:

- **Informational** — a note, not a warning. It never raises an alert on the
  device.
- **Low** — a weak signal. Worth recording, rarely worth acting on alone.
- **Medium** — worth a closer look.
- **High** — worth taking seriously and preserving.

## Why it matters

Severity exists so that attention can be spent well. Most of what Rayhunter
records is either routine or a weak signal; a smaller amount deserves real
scrutiny. Without a grading, every entry would compete equally for your alarm,
and the genuinely serious findings would be buried under the ordinary ones —
the "cries wolf" failure that makes people stop reading warnings at all, which
[Why Detection Is Hard](./concepts/why-detection-is-hard.md) describes.

But severity is Rayhunter's guess, made from the single message or pattern in
front of it, without the context only you have. A Low warning in a place with a
specific reason for concern may matter more than a High one in a lab running a
demo. Read the level as the tool's opening bid, and the interpretation page as
how to raise or lower it with what you know.

## What each level is meant to convey

- **High.** A pattern that has few innocent explanations and maps closely to a
  known attack — an identity taken without the network proving itself,
  encryption switched off, a forced move to 2G. High is the tool saying: if
  this is what it looks like, it is serious. Preserve the recording and read it
  carefully.
- **Medium.** A pattern that is more than routine but has more benign
  explanations than a High one, or that is serious only in combination. The
  continuous-location-tracking signal sits here. Medium is: look closer, gather
  context, do not yet conclude.
- **Low.** A weak signal — something worth having in the record, that on its
  own is more likely ordinary than not. A single low-severity event is the most
  common thing Rayhunter produces and the least likely to be an attack. Low is:
  noted; watch for it recurring or combining with something else.
- **Informational.** Not a warning at all. Context written into the recording —
  the [identity-exposure diary](./detectors/imsi-requested.md) entries, routine
  positioning chatter, capability exchanges — that means little alone but helps
  explain a real warning nearby. Informational events never change the device's
  status and never sound an alert.

## The design decision that shapes a whole report

Here is the part worth understanding, because it changes how you read what is
and is not in a recording. **Rayhunter never writes a row that contains only
informational events.** If, for a given message, no detector produced anything
above Informational, that message leaves no entry in the report at all.

The consequence has two sides:

- **A report is not a complete log of everything seen.** It is the warnings,
  plus the informational context that happened to land on the *same* messages
  as a warning. Long stretches of ordinary traffic are not there at all. A
  short report does not mean little was happening; it means little was flagged.
- **A detector that only ever emits informational events is invisible on its
  own.** It can only ever appear in a report riding alongside another
  detector's warning on the very same message. Two of Rayhunter's detectors are
  like this by nature, and it is why their notes show up next to warnings and
  never by themselves.

This is a deliberate choice to keep reports readable — a recording can contain
enormous amounts of routine signalling, and writing all of it would bury the
findings. But it means "absence from the report" is not the same as "did not
happen," a distinction that matters when you are trying to reconstruct what
occurred around a warning.

## The precise details

- **The levels** are an ordered type in the code (`EventType` in
  `lib/src/analysis/analyzer.rs`): `Informational = 0`, `Low = 1`,
  `Medium = 2`, `High = 3`. A row's overall severity is the maximum of the
  events on it.
- **The empty-row rule** is `AnalysisRow::is_empty` in the same file: a row is
  empty when it has no skipped-message reason and its highest event is
  Informational, and empty rows are not written. This is the mechanism behind
  the whole section above.
- **The two informational-only detectors** are the diagnostic
  ([identity-exposure diary](./detectors/imsi-requested.md) has the related
  detector; the diagnostic analyzer itself is the clearest case) and the
  incomplete-system-information check, both of which emit only Informational
  events and so never appear in a report on their own. The [detector
  reference](./detectors/index.md) marks each detector's severities.
- **On the device**, severity is shown by the status line's colour and, on
  colour displays, its pattern, so the level is legible even to someone who
  cannot distinguish the colours. [The Device Screen](./device-display.md)
  covers this.

## Where to next

[Reading Warnings Without Panicking](./concepts/interpreting-warnings.md) turns
these levels into decisions. The [Detector Reference](./detectors/index.md)
lists which levels each detector can emit.
