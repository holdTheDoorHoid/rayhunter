# Identity Requested Without Authentication

Watches for a tower that asks your phone who it is, never proves it is a real
network, and then drops the connection.

## What you would see

A warning, usually High, telling you that your phone's identity was requested
in a suspicious way — that something asked your phone for the permanent number
identifying it, without the back-and-forth that proves a request is genuine.
In the web interface it appears in the warnings for a recording; on the device,
the status line turns to the warning colour. This is the clearest single thing
Rayhunter can tell you: that the permanent identity your phone normally keeps
hidden may have been taken.

Some findings from this detector arrive at lower severity, with the warning
text itself naming a likely innocent explanation — a roaming situation, or a
SIM without an active plan. Reading that text matters, because this detector
deliberately tells you when it is less sure.

## Why it matters

The permanent number that identifies your SIM does not change, so anyone who
collects it can recognise you again anywhere, at any later time. Capturing it
from the phones in an area is the core function of the surveillance devices
called IMSI catchers, and this detector is built to catch the exact sequence
they use to do it: the identify-then-reject pattern that Dabrowski and
colleagues named *identification mode* ([Sources](#sources)).
[Capturing Your Identity](../concepts/attack-identity.md) is the full
explanation of the attack; this page is the detector that watches for it.

Because that pattern has relatively few innocent causes compared with the other
techniques, this is the detection Rayhunter can make most confidently — which
is exactly why it is the one people are most likely to over-read. The next
section is therefore as important as this one.

## When it fires harmlessly

This detector fires on real, non-attack events, and the honest list is not
short. Do not read a warning from it as proof before weighing these:

- **Aircraft coming in to land.** The best-known false positive. A phone that
  has been out of contact at altitude reconnects as the plane descends, passing
  over towers that cannot reach its home network, and reproduces the
  reconnect-and-get-rejected shape this detector watches for. It fires on
  approach roughly as a matter of course.
- **A SIM with no active plan.** When the network rejects an attach because the
  subscription is not active, the exchange can look like an identity request
  followed by a rejection. The detector recognises the specific rejection
  reason for this case and lowers the severity, and its warning text says as
  much — "likely a false positive unless your SIM card has an active plan."
- **Roaming and unfamiliar networks.** When the tower's network identity does
  not match your phone's home network, a disconnect after an identity request
  is more likely an ordinary roaming hiccup than an attack. The detector
  detects this mismatch, marks the finding Low rather than High, and says
  "could be a false positive roaming issue" in the text, naming both networks.

Two different honesty points are owed here, and keeping them apart matters.

First, this detector — inherited from upstream — **has been validated against a
real cell-site simulator.** EFF exercised it against an actual catcher, using a
test device made available through a university research group, and it fired.
That is the strongest kind of evidence in [How We Validate
Detectors](./validation.md): confirmation that it detects a genuine attack, not
only encoder-built test messages. It is why this is the detection Rayhunter can
make most confidently.

Second, and separately, **that is not the same as knowing how often it fires
when there is no attack.** A true-positive test shows the detector catches a
real catcher; it does not measure the false-positive rate in everyday traffic,
which no one can state precisely because — as [Why Detection Is
Hard](../concepts/why-detection-is-hard.md) explains — there is no field ground
truth to measure against. So the harmless cases above are still real, and a High
warning is still a strong lead to preserve rather than proof. [Reading Warnings
Without Panicking](../concepts/interpreting-warnings.md) is the method for
weighing one.

## How it works

Recall from [How Cell Networks Work](../concepts/cell-networks.md) the shape of
a normal join: your phone asks to connect, the network may ask who it is, the
network *proves itself* using your SIM's secret, encryption switches on, and
the phone is accepted. A genuine identity request sits inside a conversation
that goes on to include that proof.

This detector follows the conversation as a small state machine, tracking where
in the join sequence your phone is, and watches for an identity request that
lands in the wrong place — one that is *not* followed by the network proving
itself. The cases it treats as suspicious:

- An identity request arriving **after** authentication has already succeeded,
  where there is no ordinary reason to ask again.
- An identity request with **no preceding attach request** — the phone did not
  start a join, yet something is asking who it is.
- An identity request followed by a **disconnect with no authentication**,
  which is the identify-then-reject shape itself. Here the detector uses the
  network identities it has seen to judge: if the tower claims to be your home
  network, that is worse (High); if the identities suggest roaming, it softens
  to Low.

To make those judgements it also reads the network identity broadcast by the
tower and the one your phone names for itself, so it can tell "rejected by my
own network" from "rejected while roaming." A related companion check, the
diagnostic analyzer, quietly notes each identity-exposing message it sees as
context — those notes are informational and appear only alongside a real
warning, as [Severity](../severity.md) explains.

## Precise behavior

- **Code identifier:** `imsi_requested` (the companion is `diagnostic_analyzer`).
- **Source:** `lib/src/analysis/imsi_requested.rs`; analyzer version 4.
- **Severity:** High for the clearly suspicious transitions (identity after
  authentication; identity with no attach request; disconnect on the home
  network after an unauthenticated identity request). Low for the softened
  cases (likely-inactive-SIM rejection; likely-roaming disconnect).
  Informational for an identity request that times out with no follow-up
  after 50 messages.
- **Deduplication:** it is a state machine, not a per-message test; it emits one
  event per suspicious transition and resets its timeout on a successful
  authentication. It does not re-fire continuously on a single stuck state.
- **What it deliberately ignores:** a connection-release message is *not*
  currently treated as the disconnect that completes the pattern. That path is
  deliberately disabled in the code because it produced a second, duplicate
  warning on false positives; the maintainers left a note to revisit it. The
  practical effect is a bias toward fewer false alarms here, at the cost of
  missing a reject delivered specifically by connection release.
- **Validation:** inherited from upstream and exercised by two of Rayhunter's
  demonstration scenarios (identity demanded after authentication, and identity
  demanded with no attach request). Beyond that, EFF has validated it against a
  real cell-site simulator (a test device obtained through a university research
  group), where it fired on a genuine attack — the real-capture level in [How We
  Validate Detectors](./validation.md). That establishes it detects a real
  catcher; it does not establish a field false-positive rate, which remains
  unknown for the reasons in the harmless-cases section.
- **Note on upstream drift:** the upstream project has recent work to read your
  home network identity from the SIM card directly, which would sharpen the
  roaming judgement above. That is not yet in this fork, and this page
  describes the fork's current behavior.

## Configuration

Enabled by default. The configuration key is `imsi_requested` under
`[analyzers]` in `config.toml`, or the toggle labelled "Identity requested
without proof of identity" on the settings page. The companion diagnostic
analyzer is the separate `diagnostic_analyzer` key. Turning `imsi_requested`
off removes Rayhunter's clearest identity-capture warning, so leave it on unless
you have a specific reason not to. [Configuration](../configuration.md) covers
how analyzer toggles are applied.

## Sources

- **The attack.** Dabrowski et al., "IMSI-Catch Me If You Can," ACSAC 2014 —
  the identification-mode pattern this detector targets. EFF's white paper
  *Gotta Catch 'Em All* for the mechanism. Both in
  [Sources and Further Reading](../references.md).
- **The protocol.** 3GPP TS 24.301 (NAS for EPS): the Identity Request and
  Response messages, the Authentication and Security Mode Command steps whose
  absence is the tell, and the Attach Reject causes the detector reads to
  distinguish an inactive SIM from an attack.
- **In this book.** [Capturing Your Identity](../concepts/attack-identity.md)
  for the attack, [Reading Warnings Without
  Panicking](../concepts/interpreting-warnings.md) for how to weigh a warning.
