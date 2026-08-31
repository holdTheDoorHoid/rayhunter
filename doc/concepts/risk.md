# Legal and Personal Risk

Using Rayhunter means running a piece of software, carrying a small radio
device, and sometimes sharing what you find. Each of those can carry risk, and
the risk depends heavily on where you are and who you are. This page is here to
help you see the shape of those questions clearly enough to ask the right ones.

It does not answer them. **Nothing here is legal advice, and this page cannot
know your situation.** Laws differ by country and change over time, and the
same act can be unremarkable in one place and serious in another. Where the
stakes are real for you, the move is to put these questions to someone
qualified in your jurisdiction — not to rely on a documentation page, however
carefully written. What follows is a map of what to think about, so that
conversation is a productive one.

## What you would see

Risk is the part of using this tool with no indicator light. Rayhunter will not
warn you that a recording contains something sensitive, that carrying the
device somewhere raises your exposure, or that a phrasing you are about to
publish claims more than you can defend. Those judgements are entirely yours,
which is why it helps to have thought about them before you are in the moment.

## Why it matters

The people most likely to want Rayhunter — journalists, organizers,
researchers, people who already have reason to think about surveillance — are
often the people for whom a misjudgement carries the most weight. Getting the
technical detection right and the human side wrong is a real way to end up
worse off than before. Treating the risk questions with the same seriousness as
the warnings is part of using the tool well.

## The three activities and the questions they raise

### Running the software

Rayhunter is a passive receiver. It listens to signalling its own modem already
handles and records it; it transmits nothing extra and does not connect to
anyone else's equipment. The upstream project's position, which this fork
inherits, is that running it is believed not to violate US law
([the disclaimer page](../disclaimer.md) carries the full statement). "Believed
not to" is not "certainly does not," and it is a statement about the United
States.

The questions to carry:

- **Where are you?** Radio monitoring, recording signalling, and possessing the
  equipment to do so are regulated differently across countries, and some treat
  them far more strictly than the US does. If you are outside the US, this is
  the first thing to check with local counsel, not an afterthought.
- **What does the recording contain?** A capture can include identifiers — your
  own, and potentially fragments relating to other devices the radio saw. Think
  about what you are storing and for how long, the way you would with any
  sensitive data.

### Carrying the device

A Rayhunter is a small hotspot-style device. Carrying it is ordinarily
unremarkable, but context changes that:

- **Where you take it.** Borders, secured facilities, courthouses, and some
  event venues have their own rules about electronics, and their own scrutiny.
  A device full of recorded radio signalling is a thing you may be asked to
  explain. Crossing a border with it is a specific situation worth thinking
  through in advance, including that border searches of devices operate under
  different rules than searches elsewhere.
- **What it says about your activity.** In some settings the mere fact of
  carrying detection equipment may draw attention. Only you can weigh whether
  that matters for you.

### Publishing what you find

Sharing a finding is where the technical and the personal meet, and where an
avoidable mistake does the most damage. Two distinct risks:

- **Overstating the claim.** [Reading Warnings Without
  Panicking](./interpreting-warnings.md) covers this in detail: a warning is a
  pattern consistent with an attack, not proof of one, and not an
  identification of who. Publishing it as more than that can mislead people and
  can expose you to challenge when the stronger claim cannot be backed up. Say
  what the tool observed, and no more.
- **Exposing identifiers.** A recording or screenshot can contain permanent
  identifiers — IMSI, IMEI, temporary identities, location. Publishing those,
  yours or anyone else's, is its own harm and possibly its own legal question.
  [Sharing What You Find](../sharing-findings.md) covers how to redact before
  anything leaves your hands. Redact first; share second.

## How to think about it, in one frame

A useful order of questions, before acting on anything Rayhunter gives you:

1. **Jurisdiction.** What do the laws where I am actually say about running
   this, carrying this, and recording this? If I do not know, who does?
2. **Data.** What is in this recording, whom does it concern, and what is my
   plan for keeping or deleting it?
3. **Claim.** If I share this, exactly what am I asserting, and can I defend it
   against someone hostile? Have I removed identifiers?
4. **Help.** Given my actual situation, is this the point to involve a lawyer,
   an editor, a security-minded colleague, or an organization that supports
   people in my position?

None of these has a universal answer, and that is the point. The page's job is
to make sure you ask them.

## The precise details

- **The tool's technical posture** — passive, transmits nothing — is described
  in [How Cell Networks Work](./cell-networks.md) and
  [What This Tool Cannot Tell You](./limitations.md). It is relevant to the
  legal picture but does not settle it: passivity is not a universal legal
  defence.
- **The inherited legal statement** lives on [the disclaimer
  page](../disclaimer.md); this page is the plain-language companion to it, not
  a replacement. Where they seem to differ, the disclaimer is the formal text.
- **Redaction mechanics** — what a recording contains and how to remove
  identifiers before sharing — are in [Sharing What You
  Find](../sharing-findings.md).
- **This page gives no legal advice and creates no reliance.** It is
  orientation. Your jurisdiction, and a professional in it, are the authority.

## Where to next

That is the last of the Understanding section. If you have not yet, [Reading
Warnings Without Panicking](./interpreting-warnings.md) is the page to keep
close once warnings start appearing. To get a device running, head to
[Choosing a Device](../supported-devices.md) and the
[Quick Start](../quick-start.md).
