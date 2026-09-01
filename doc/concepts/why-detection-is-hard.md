# Why Detection Is Hard

Rayhunter, and every tool like it, is trying to do something genuinely
difficult, and it is better to understand why than to be surprised by it
later. A detector that seems to say less than you hoped is not necessarily
broken. It may be telling the truth about a hard problem.

This page explains the four reasons catching a cell-site simulator is hard,
none of which is a flaw in any particular tool. They are properties of the
situation itself. Understanding them is what makes the difference between
trusting Rayhunter too much and trusting it about the right amount, which is
the whole subject of the next page,
[Reading Warnings Without Panicking](./interpreting-warnings.md).

## What you would see

The symptom of these difficulties, in daily use, is warnings that are real
signals but not proof, and quiet stretches that are reassuring but not a
guarantee. Both are honest outputs of a hard problem, and both are
misread often if you do not know why they look the way they do.

## Why it matters

If you expect certainty from Rayhunter, two failures follow, and both are
harmful. You might treat every warning as confirmation you were targeted, and
act on something that was a misconfigured tower. Or, after a few warnings turn
out to be nothing, you might stop believing them and miss a real one. The
research on detection apps names both failure modes directly, and they are the
reason this page comes before you read a single warning in anger.

The goal is not maximum suspicion or maximum trust. It is *appropriate*
trust, believing the tool exactly as much as its evidence supports on that
occasion. Getting there requires knowing what stands in the way.

## How it works

### One: a passive tool sees only its own mail

Rayhunter is on the network as an ordinary device, and it watches what the
network sends *to it*. That is its great virtue, it is cheap, silent, carries
no risk of transmitting anything, and cannot itself be easily detected. It is
also a hard limit. Rayhunter does not see what a tower says to the phone next
to yours, cannot observe a device that is only listening and never transmits,
and learns nothing about an attack that never targets the Rayhunter device
itself. It sees one vantage point, honestly and completely, and nothing
beyond it. [What This Tool Cannot Tell You](./limitations.md) is the full
accounting of that boundary.

### Two: there is no answer key

To know for certain how good a detector is, you would need a list of where and
when cell-site simulators actually operated, to check its warnings against.
No such list exists. These deployments are secret by design. The richest
public knowledge of how, say, US government agencies use them comes from
leaked documents, public-records requests, and things surfaced in court, not
from any published registry (the SeaGlass project makes exactly this point
about why distributed detection is needed at all;
[Sources and Further Reading](../references.md)).

The consequence runs deep. Without an answer key, no one can state a precise
false-positive rate for most of these detectors, because computing one would
require knowing which past warnings were real. When a detector page in this
book says the true false-positive rate is not known, that is not an evasion.
It is a direct result of there being no ground truth to measure against, and
saying so plainly is more useful than inventing a number.

### Three: honest networks look guilty all the time

The detectors work by spotting artifacts, the traces a catcher tends to
leave. But real networks, run by real people under real budget and coverage
pressure, produce many of the same traces without any ill intent. Towers are
misconfigured. Old equipment behaves oddly. A phone reconnecting after a
flight, a border crossing, a stretch of no coverage, or a roaming agreement
can produce exactly the sequence a detector watches for. The
[downgrade page](./attack-downgrade.md) gives the sharpest example: in much of
the world, the very thing that would be a red flag in one country is an
ordinary Tuesday in another.

So most detectors cannot cleanly separate "attack" from "messy but honest
network." They separate "worth a look" from "nothing unusual," which is a
weaker and more truthful claim.

### Four: the target moves

The people who operate these devices can read this documentation too. Once a
detection method is public, a well-resourced operator can adjust to avoid the
artifact it keys on, behave a little more like a normal network, skip the
step that gives them away. Detection and evasion push against each other over
time, and a method that worked well last year may be quietly worked around
this year. This is not a reason for despair; it is a reason not to read silence
as safety, and to value several independent signals over any single clever one.

### What all four have in common

The deepest evidence for how hard this is comes from testing the detectors
against a real, built catcher. When researchers did exactly that, they found
that apps implementing nominally the *same* detection methods did not even
agree with each other on whether a given tower was suspicious (the
White-Stingray study, [Sources and Further Reading](../references.md)). If
equally serious tools disagree on the same tower, then no single warning from
any one of them can be treated as the last word. That finding is the
foundation under everything the next page tells you about how to read a
Rayhunter warning.

## The precise details

- **The passive-observer limit** is a property of Rayhunter's design: it reads
  the diagnostic stream from its own modem, the control messages its own radio
  received. [How Cell Networks Work](./cell-networks.md) describes that
  vantage point and [What This Tool Cannot Tell You](./limitations.md) its
  edges.
- **The no-ground-truth problem** is why this book's honesty rules
  (`STYLE.md` §5) require every detector page to state its false positives and
  to distinguish tested from untested behavior. Several detectors here have
  been exercised only against reference-encoder vectors or synthetic data, not
  real captures; those pages say so in plain text, and
  [How We Validate Detectors](../detectors/validation.md) explains the levels
  of evidence.
- **The disagreement finding** is Park et al., "White-Stingray," USENIX WOOT
  2017; **the deviation-from-normal method and the secrecy of deployment data**
  are Ney et al., "SeaGlass," PoPETs 2017, both in
  [Sources and Further Reading](../references.md). The framing of appropriate
  trust over maximum trust comes from the human-factors literature cited there
  (Lee & See, 2004).

## Where to next

[Reading Warnings Without Panicking](./interpreting-warnings.md) takes
everything on this page and turns it into a method: how to weigh a specific
warning, what raises and lowers your confidence, and what to actually do at
each severity level.
