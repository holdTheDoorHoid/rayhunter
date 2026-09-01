# Sharing What You Find

Sharing a recording is how a finding becomes something others can verify, and how
patterns across many people get noticed. It is also how identifiers and
overstated claims escape into the world. This page covers both: what a recording
actually contains, how to share it carefully, and how to describe a finding
without claiming more than it supports.

It pairs with [Reading Warnings Without
Panicking](./concepts/interpreting-warnings.md), which is about reading a warning;
this page is about what happens after, when you decide to tell someone.

## What a recording contains

A Rayhunter recording is the raw signalling your device saw, plus the analysis of
it. That raw signalling can contain **real identifiers**, and you should assume it
does before sharing:

- Your device's own permanent identity (IMSI, IMEI) and temporary identity, if
  they appeared in the traffic.
- Cell identities and network identifiers for the towers around you.
- Timing, and — if you had GPS configured — location.

These are exactly the things this whole tool exists to protect. A recording is
useful evidence *because* it is detailed, and that same detail is what makes
sharing it carelessly a real exposure.

## The bundle describes itself

Every download includes a `metadata.json` alongside the capture, saying what
produced it and what has been read from it:

- The Rayhunter version that **made** the recording and the version that
  **exported** it, which are not always the same.
- The device type, operating system and architecture.
- The recording's id, your name and notes for it, when it started, when its last
  message arrived, and how big it is.
- **Which detectors ran, and at what version.**

That last one matters more than it looks. A report with no warnings means
something quite different depending on whether the detector that would have
caught the thing existed yet. Without it, somebody reading your capture months
later has no way to tell "nothing happened" from "nothing was looking".

The sidecar deliberately contains **no identifiers**: no IMSI, IMEI or temporary
identity, and no passwords or WiFi details. It is meant to be the safe part of
the bundle. The capture next to it is not, which is the rest of this page.

## The shareable download

Every recording offers two downloads. The ordinary **zip** is the full evidence
bundle, raw capture included. The **zip (shareable)** one is meant to be sent to
somebody:

- The device's own identifiers are removed from the capture on the way out. The
  digits are set to zero rather than the field being deleted, so the messages
  still open in Wireshark and you keep the evidence of *what happened* while
  losing *who it was*.
- **The raw recording is left out entirely.** Nothing removes identifiers from
  the QMDL, so including it would hand over exactly what the bundle claims to
  have taken out.
- A `redaction-report.json` says how many identifiers were removed and how many
  messages were examined.

Your original recording is never modified. Redaction happens as the download is
built, because a recording is evidence and evidence that got quietly rewritten
is not evidence any more.

### What it does not promise

**This is a real reduction in exposure, not a guarantee of cleanliness.** It
finds identities in the messages where a device announces itself and they sit at
a known position: identity responses and attach requests. An identity carried
somewhere it does not look for would survive, and a ciphered message hides its
contents from Rayhunter as much as from anyone reading the capture.

That is why the bundle reports counts rather than declaring itself clean.
Believing a capture is safe when it is not leaves you worse off than knowing it
is not. If what you are sharing is genuinely sensitive, look at the redacted
PCAP yourself before sending it.

The capture still contains things the redaction does not touch, and you should
decide about them deliberately:

- Cell identities and network identifiers for the towers around you, which say
  where you were.
- Timing, and location if you had GPS configured.

## Where to report

What to do with a verified finding depends on your situation:

- **To contribute to public knowledge**, the upstream Rayhunter project and its
  community are where distributed findings are gathered — see [Support, Feedback,
  and Community](./support-feedback-community.md). Send the least identifying
  useful information, and never a recording containing demo data.
- **If your situation is sensitive**, consider whether a lawyer, an editor, a
  security-minded colleague, or an organization that supports people in your
  position should see it before anyone else. [Legal and Personal
  Risk](./concepts/risk.md) covers the shape of that decision.

## How to describe a finding without overstating it

This is the part that outlives the recording. If you repeat a finding — to a
colleague, a reporter, or an audience — the words matter as much as the data,
because an overstated claim that collapses discredits the real work and misleads
people who trusted you. The full guidance is in [Reading Warnings Without
Panicking](./concepts/interpreting-warnings.md); the essentials:

- Say what the tool observed: "Rayhunter flagged a pattern consistent with
  [technique], and I have the recording."
- Name it as consistent-with, not proof-of, and do not attribute it to any actor.
- Offer the capture (redacted as above) so the claim can be checked, rather than
  asking to be believed.

Avoid the claims the tool cannot support: that a specific device was present, that
a named party operated it, that you personally were targeted, or that anything is
certain. [What This Tool Cannot Tell You](./concepts/limitations.md) is the list
of what no recording can establish.

## Where to next

- [Reading Warnings Without Panicking](./concepts/interpreting-warnings.md) — the
  full method for weighing and describing a finding.
- [Legal and Personal Risk](./concepts/risk.md) — before you publish anything.
- [Support, Feedback, and Community](./support-feedback-community.md) — where
  findings are gathered.
