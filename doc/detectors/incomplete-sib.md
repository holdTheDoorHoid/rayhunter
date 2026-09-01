# Incomplete System Information

Watches for towers that broadcast far less information about themselves than a
real one does.

## What you would see

On its own, nothing in the warnings list — and this is the most important thing
to understand about this detector. It produces only an *informational* note,
never a warning, so by itself it never turns the device status line or appears
as a warning in a recording. What it produces is context: a quiet note, written
into the recording only when it lands on the same message as another detector's
warning. [Severity, and What It Means](../severity.md) explains why
informational-only findings behave this way.

So you will see this detector's contribution not as an alarm of its own, but as
supporting detail next to a warning that something else raised — most usefully
next to an [identity request](./imsi-requested.md).

## Why it matters

A genuine tower continuously broadcasts a series of information blocks
describing itself, its neighbours, and how to use it. A fake tower often sends
only the first block or two. The rest takes effort to produce and brings
whoever is operating it no benefit, because they only need your phone to
connect briefly — long enough to [take its identity](../concepts/attack-identity.md)
and let it go. A tower that announces almost nothing about itself is behaving
the way a catcher that cannot be bothered to impersonate a full network
behaves.

On its own, that is weak evidence — plenty of honest reasons produce a thin
broadcast too. Its value is as corroboration. A thin broadcast *and* an identity
request together is a more coherent story than either alone, which is exactly
why this detector is built to sit quietly in the record until a real warning
appears beside it.

## When it fires harmlessly

This note appears for benign reasons often, which is part of why it is
informational rather than a warning:

- **Honest but minimal or misconfigured towers.** A real tower can broadcast a
  short scheduling list for ordinary reasons of configuration or coverage,
  without anything being wrong.
- **Weak or partial reception.** If your device caught only part of a tower's
  broadcast cycle, the information can look incomplete when the tower actually
  sent more.
- **Normal variation.** Broadcast contents vary between networks and equipment,
  and a shorter list is not by itself abnormal.

Because it never raises a warning on its own, there is little risk of this
detector alarming you falsely — its danger, if any, is the opposite: being
read as more damning than it is when it does appear beside a warning. Treat it
as one supporting detail among several, not a finding in itself. No measured
rate is recorded for how often a thin broadcast is benign versus suspicious,
and there is no reliable way to establish one.

## How it works

The first information block a tower broadcasts (SIB1) includes a schedule
promising the other blocks that follow it. This detector reads that first block
and checks how many further blocks the schedule lists. When the schedule is
very short — fewer than two scheduled entries — it records its informational
note that the broadcast looks incomplete. It does not inspect the contents of
the later blocks or judge intent; it looks at the length of the promise in the
first one.

## Precise behavior

- **Code identifier:** `incomplete_sib`.
- **Source:** `lib/src/analysis/incomplete_sib.rs`; analyzer version 2.
- **Severity:** Informational only. It emits no Low, Medium, or High event under
  any condition, so — per [Severity](../severity.md) — it never appears in a
  report on its own, only alongside another detector's warning on the same
  message.
- **What it inspects:** the SIB1 scheduling-information list; it flags a list
  with fewer than two entries.
- **Deduplication:** none; each qualifying SIB1 produces the note.
- **What it deliberately ignores:** the actual contents and validity of the
  later blocks, and any correlation with other detectors (that correlation
  happens when a reader reads the report, not inside this detector).
- **Validation:** inherited from upstream. No demonstration scenario or
  real-capture validation for it is recorded in this repository.
- **A note on presentation.** The settings-page description presents this check
  in the same terms as the warning-raising detectors and does not mark it as
  informational-only. In the code it is informational-only. This page describes
  the code's actual behavior; the discrepancy is noted for maintainers to
  reconcile.

## Configuration

Enabled by default. The key is `incomplete_sib` under `[analyzers]`, or the
"Tower broadcasting only a fragment of its details" toggle on the settings
page. Because it only ever adds informational context, leaving it on costs
nothing in warning noise. [Configuration](../configuration.md) covers applying
analyzer toggles.

## Sources

- **The mechanism.** EFF's white paper *Gotta Catch 'Em All* on how catchers
  present themselves — [Sources and Further Reading](../references.md).
- **The protocol.** 3GPP TS 36.331 (E-UTRA RRC): System Information Block Type 1
  and its scheduling information list.
- **In this book.** [What a Cell-Site Simulator
  Is](../concepts/cell-site-simulators.md) for why a fake tower cuts corners,
  and [Identity Requested Without Authentication](./imsi-requested.md), the
  warning this note most often supports.
