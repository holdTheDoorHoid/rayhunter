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
- Timing, and, if you had GPS configured, location.

These are exactly the things this whole tool exists to protect. A recording is
useful evidence *because* it is detailed, and that same detail is what makes
sharing it carelessly a real exposure.

## There is no one-click redaction

Be clear-eyed about this: **Rayhunter does not have a feature that scrubs
identifiers out of a recording for you.** The display name you can set is
restricted so it is safe in a filename, but that is not redaction of the capture
itself. So sharing safely is a matter of deliberate choice, not a button:

- **Share the least that makes your point.** Often the analysis, which detector
  fired, at what severity, when and where in general terms, is enough to raise
  the alarm or ask for help, without handing over the raw capture and every
  identifier in it.
- **Decide who genuinely needs the raw recording.** A trusted expert verifying a
  finding may need it; a public post almost never does. Sharing raw with one
  person you trust is a different act from publishing.
- **Screenshots leak too.** A screenshot of the interface can contain your IMSI,
  IMEI, temporary identity, cell identities, and location. Crop or cover them
  before posting an image, and check the whole frame, not only the part you meant
  to show.
- **Never share a recording that contains demo data as if it were real.** The
  demo writes a clearly-labelled fake warning into the current recording; a
  recording containing it is not evidence and must not be sent to EFF or
  presented as a detection. Start a fresh recording after any demo. See [Your
  First Warning](./first-warning.md).

If you are unsure whether something in a recording identifies you or someone
else, treat it as if it does.

## Where to report

What to do with a verified finding depends on your situation:

- **To contribute to public knowledge**, the upstream Rayhunter project and its
  community are where distributed findings are gathered, see [Support, Feedback,
  and Community](./support-feedback-community.md). Send the least identifying
  useful information, and never a recording containing demo data.
- **If your situation is sensitive**, consider whether a lawyer, an editor, a
  security-minded colleague, or an organization that supports people in your
  position should see it before anyone else. [Legal and Personal
  Risk](./concepts/risk.md) covers the shape of that decision.

## How to describe a finding without overstating it

This is the part that outlives the recording. If you repeat a finding, to a
colleague, a reporter, or an audience, the words matter as much as the data,
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

- [Reading Warnings Without Panicking](./concepts/interpreting-warnings.md), the
  full method for weighing and describing a finding.
- [Legal and Personal Risk](./concepts/risk.md), before you publish anything.
- [Support, Feedback, and Community](./support-feedback-community.md), where
  findings are gathered.
