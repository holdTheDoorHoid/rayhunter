# Reading Warnings Without Panicking

A warning has appeared. Your heart rate is up, and you want to know one thing:
does this mean someone is after me?

The honest answer is that a single warning almost never means that on its own,
and this page is here to help you get from the warning to a fair judgement of
what it is worth, without talking yourself into a certainty the evidence does
not support, and without dismissing something that deserves a second look.
Both of those mistakes are common, and both do real harm. Someone who reads
every warning as proof of targeting may abandon a phone they needed or make a
public claim they cannot stand behind. Someone who decides the tool cries wolf
may switch it off the week it would have mattered. The aim is neither. The aim
is to believe each warning as much as it earns.

If you are holding your first warning right now and want to be walked through
the screen in front of you, [Your First Warning](../first-warning.md) does
that step by step. This page is the thinking behind it.

## What a warning is, and what it is not

A Rayhunter warning says: **a pattern consistent with a known attack technique
appeared in the traffic this device saw.** That is a real, specific
statement, and it is worth taking seriously.

It is also not several things, and naming them is the fastest way to steady
yourself:

- It is **not proof** that an attack happened. As
  [Why Detection Is Hard](./why-detection-is-hard.md) explains, honest but
  messy networks produce the same patterns, and there is no answer key that
  would let anyone say for sure.
- It does **not identify who** did anything. Rayhunter sees the technique in
  the traffic, not a hand behind it. It cannot name a person, an agency, or a
  motive.
- It is **not about you specifically** by default. Many techniques sweep
  everyone in an area. A warning rarely means *you* were singled out, and
  usually cannot distinguish that from having been near something.

Hold those three in mind and a warning becomes what it actually is: a lead to
weigh, not a verdict to act on.

## Why base rates matter more than the warning

Here is the piece most people miss, and it is the single most important idea
on this page. **How likely a warning is to be real depends far more on how
common real attacks are where you are than on how good the detector is.** A
very accurate detector can still produce mostly false alarms, if the thing it
looks for is rare. This is not a knock on the detector; it is arithmetic, and
it is worth walking through with real numbers, because the result surprises
almost everyone the first time.

### A worked example

Suppose you are in a city with a lot of phones and very few cell-site
simulators. Let us put numbers on it, round ones, chosen to make the
arithmetic clear, not because they are measured:

- Imagine **10,000** connection sequences your device might see over some
  period.
- Real attacks are rare here: say **1 in 1,000** of those sequences is an
  actual cell-site simulator. That is **10** real attacks out of the 10,000.
- The other **9,990** are ordinary network behaviour.

Now suppose the detector is **good**:

- It catches **90%** of real attacks. So of the 10 real ones, it flags
  **9**. (It misses 1.)
- It has a **low** false-alarm rate: it wrongly flags only **1%** of ordinary
  sequences. One percent sounds small, but 1% of 9,990 is about **100**
  false alarms.

So the detector raises about **9 + 100 = 109** warnings. Of those, only **9**
are real. Work out the share that are true:

> 9 real ÷ 109 total ≈ **8%.**

**About 8 warnings in 100 point at a real attack, and the other 92 are
something else, with a detector that is 90% sensitive and only 1% wrong.**
That is the base-rate effect. When the thing you are hunting is rare, even a
strong detector produces far more false alarms than true ones, because there
is so much more ordinary traffic for the small error rate to act on.

Two things follow, and they pull in opposite directions, which is the whole
point:

- **A single warning, in a low-risk setting, is probably not an attack.** Do
  not panic at one.
- **The warning still moved the odds.** Before it, your chance was 1 in 1,000.
  After it, roughly 1 in 12. The warning is not nothing, it is a reason to
  look closer, gather more, and raise your attention. It is not, by
  itself, a conclusion.

And if you are somewhere real attacks are genuinely more likely (a targeted
protest, a border, a place with a known history), the base rate rises and so
does the share of warnings that are real. The same warning means more there
than it does on your sofa. Context is not a detail; it is most of the answer.

## What raises your confidence

A lone warning is weak. Confidence comes from things stacking up. Each of
these, when present, makes a real attack a better explanation:

- **Repetition in the same place.** The same warning every time you pass one
  particular spot is harder to explain as random misconfiguration than a
  one-off.
- **Several independent detectors firing together.** An identity request *and*
  a downgrade *and* a null cipher, close together, is a coherent story, one
  technique setting up the next, as [How These Attacks Actually
  Work](./attacks.md) describes. Several unrelated detectors agreeing is much
  stronger than any one alone.
- **Correlation with a real event.** A warning that lines up with something
  you can point to (a specific gathering, a specific time, a place you had
  reason to expect attention) carries more weight than one from an ordinary
  afternoon.
- **A clean capture someone else can check.** A recording, with the messages
  intact, that another person with the skills can examine and agree on. This
  is the difference between "my phone buzzed" and evidence.

## What lowers it

Equally important, and more often relevant. Each of these makes a false alarm
the better bet:

- **A single, low-severity event.** One low warning and nothing else is the
  most common thing Rayhunter produces, and the least likely to be an attack.
- **A detector known to be noisy.** Some detectors fire more loosely than
  others by design; each detector page states its own false-positive picture,
  and the honest ones say plainly where the rate is unknown.
- **Flying, or crossing a border.** A phone reconnecting after time out of
  contact (coming in to land, clearing customs) reproduces some of the
  exact patterns a catcher does. The identity detector is documented to fire
  on aircraft on approach for exactly this reason.
- **Roaming or an unfamiliar network.** Away from your home network, on
  carriers your phone does not usually see, benign oddities multiply and more
  of them look suspicious.
- **A lab or test setup.** If you or anyone nearby is running test equipment,
  a demo, or the tool's own demonstration feature, treat warnings as noise
  until you have ruled that out.

There is a deeper reason to keep this list in view. Researchers who tested
detection apps against a real, built catcher found that apps using nominally
the same methods disagreed with each other about whether a given tower was
suspicious (the White-Stingray study,
[Sources and Further Reading](../references.md)). If serious tools disagree on
the same tower, then any one warning, including Rayhunter's, is one opinion,
not the final word.

## What to actually do, by severity

Concrete steps, not "consult an expert." Severity is Rayhunter's own estimate
of how much a finding is worth; [Severity, and What It
Means](../severity.md) defines the levels precisely.

- **Low.** Note it and carry on. Do not change your behaviour on one low
  warning. If low warnings recur in the same place, or start arriving
  alongside other detectors, it graduates, keep the recording and start
  paying attention to where and when.
- **Medium.** Look closer. Open the recording, see which detector fired and
  what else happened around it ([The Packet Explorer](../packet-explorer.md)
  shows the actual messages). Note where you were and what was happening. One
  medium warning is a reason to gather information, not yet to act on a
  conclusion.
- **High.** Take it seriously and preserve the evidence. Save the recording
  before it rotates away. Write down the place, the time, and the
  circumstances while they are fresh. Consider whether your surroundings
  offered a plausible reason. If your situation is genuinely sensitive, this
  is the point to bring in someone who can help you read it and to think
  about [the personal and legal side](./risk.md), but preserve first, decide
  second.

At every level, the strongest move is the same: **keep the recording.** It is
the difference between a feeling and something another person can verify, and
verifiability is what separates a defensible finding from an anxious guess.

## How to talk about a finding without overstating it

If you might repeat a finding publicly (as a journalist, an organizer, anyone
with an audience), the words you choose matter as much as the finding, because
an overstated claim that falls apart discredits the real work and can mislead
people who trusted you. Rayhunter gives you a pattern in traffic. Say exactly
that, and no more.

Defensible:

- "Rayhunter flagged a pattern consistent with an identity-capture attack at
  this location, and I have preserved the recording."
- "The device recorded several detectors firing together during the event; I
  am sharing the capture so others can examine it."
- "This is consistent with a cell-site simulator. It is not proof one was
  present, and it does not identify who might have operated it."

Not defensible:

- "Rayhunter detected a Stingray at the protest." (Claims proof and a specific
  device the tool cannot confirm.)
- "I was targeted by police surveillance." (Names a who and a motive the tool
  cannot see, and asserts you specifically were singled out.)
- "My phone was definitely hacked." (Certainty the evidence does not support,
  and the wrong mechanism besides.)

The pattern in the good examples is the same: state what the tool observed,
name it as consistent-with rather than proof-of, avoid attributing it to any
actor, and offer the recording so the claim can be checked. That posture is
not timidity. It is what makes you credible the day you have a finding that
really matters.

## The stance to aim for

Think of Rayhunter as a competent colleague handing you an instrument reading,
not an oracle delivering a verdict. The instrument is honest and the reading
is real, but what it means is a judgement, yours to make, with the context
only you have. The colleague's manner is the one to borrow: not reassuring you
that everything is fine, not alarming you that everything is dire, only telling
you what the instrument can and cannot support and leaving the decision where
it belongs.

## Where to next

[What This Tool Cannot Tell You](./limitations.md) draws the outer boundary, the things no warning, however you read it, can speak to. And
[Legal and Personal Risk](./risk.md) covers the human side of acting on a
finding.
