# The Packet Explorer

The packet explorer lets you browse the individual messages inside a recording, the actual signalling your device exchanged with the network, and jump straight
to the ones that raised a warning. It is how you check a finding for yourself
rather than taking the analysis on faith. It is an addition in this fork.

## What it is for

A warning tells you a detector reacted to something. The packet explorer shows
you the something. When a real warning appears, being able to open the exact
message behind it, and see the messages around it, is the difference between "the
tool flagged this" and evidence you or a trusted expert can examine. [Your First
Warning](./first-warning.md) has you practise this on a harmless demo warning
before it matters.

## Reaching it

Open a recording from the [history](./web-interface.md), and the packet explorer
is part of the recording's view. It lists the messages in that recording in
order, each with its number, so any single message can be referred to precisely.

## What you can do

- **Browse the messages** in the order they were recorded, each decoded by the
  same path the detectors use, so what you see is what the detector saw, not a
  separate re-interpretation.
- **Filter** the list to the kinds of message you care about, to cut through the
  volume of routine signalling a recording contains.
- **See which messages raised a warning.** Messages that produced a finding carry
  a severity badge, so you can pick them out of the stream at a glance.
- **Jump to a packet by number.** A recording can hold a great many messages;
  the jump box takes you straight to a specific one, including the exact message
  a warning names, since each finding records its packet number.
- **Always see the alert messages.** Messages that raised a warning stay visible
  even when a filter would otherwise hide them, so a filter can never
  accidentally conceal the thing you are investigating.

## Reading a message

Opening a message shows its decoded detail. For a message that raised a warning,
this is where you confirm what the detector reacted to, the specific field or
value that matched its pattern. Cross-reference with the relevant [detector
page](./detectors/index.md), whose "How it works" and "Precise behavior"
sections describe exactly what that detector reads.

Remember the honest frame from [Reading Warnings Without
Panicking](./concepts/interpreting-warnings.md): seeing the message confirms
*what happened*, which is not the same as confirming *what it means*. A message
can be exactly what a detector flags and still have an innocent explanation.

## It carries a report format change

Recording which exact message produced each finding is what advanced Rayhunter's
analysis report to a newer format version (version 3). This is mostly invisible
to you, but it matters in one case: moving recordings between this fork and
upstream. A report written here carries the per-message number that older
versions did not, so [Compatibility With Upstream](./fork/compatibility.md)
covers how to handle that, the short version is to re-analyse the raw recording
on whichever version you are using, which regenerates the report cleanly.

## Where to next

- [Detector Reference](./detectors/index.md), what each detector reads, to
  cross-reference against a flagged message.
- [Sharing What You Find](./sharing-findings.md), how to export and redact a
  recording before showing it to anyone.
- [Report Format](./report-format.md), the report the explorer draws on, by
  version.
