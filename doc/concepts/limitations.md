# What This Tool Cannot Tell You

A detector is only trustworthy if you know where its knowledge ends. This page
is the honest edge of Rayhunter: the things it cannot see, cannot prove, and
was never built to do. None of these is a bug or a missing feature to be added
later. They follow from what Rayhunter is, a small, passive device listening
to its own slice of the network, and knowing them is what keeps a warning
from being read as more than it is.

This is written as a plain list of limits, not as a legal disclaimer. The
legal disclaimer is [its own page](../disclaimer.md); this one is about what
the instrument can physically know.

## What you would see

The limits below do not announce themselves. Rayhunter shows you what it found;
it does not show you the shape of what it could never have found. That absence
is invisible by nature, which is exactly why it has to be written down. A quiet
Rayhunter means "I saw nothing I flag," not "nothing happened."

## Why it matters

Every limit here is a way a warning could be over-read, or a silence
over-trusted. If you think Rayhunter can name who ran a device, you may accuse
the wrong party. If you think its quiet proves you are safe, you may relax when
you should not. Reading the tool correctly, the subject of [Reading Warnings
Without Panicking](./interpreting-warnings.md), depends on holding these
boundaries in mind, so they are collected here in one place to point back to.

## The limits

### It cannot tell you who is behind anything

Rayhunter sees techniques in traffic. It does not see a person, an agency, a
company, or a motive. A warning can say "this looks like an identity-capture
attack." It can never say who operated the device, why, or whether you in
particular were the reason. Any name attached to a Rayhunter finding was added
by a human drawing an inference, it did not come from the tool, and it does
not carry the tool's authority.

### It cannot prove an attack happened

This is the throughline of [Why Detection Is Hard](./why-detection-is-hard.md)
and it belongs on this list too. A warning is a pattern consistent with an
attack, not a confirmed attack, because honest networks produce the same
patterns and there is no ground truth to settle it against. Rayhunter can raise
your suspicion and preserve evidence. It cannot deliver a verdict, and it does
not try to.

### It sees only what is sent to this device

Rayhunter reads the control messages its own modem received. It does not see
what a tower says to the phone in your other pocket, let alone to strangers
nearby. It cannot survey an area, count how many phones a device caught, or
tell you whether your neighbour was affected. One device, one vantage point.
Everything it reports is about the traffic *this* Rayhunter's radio saw, and
nothing beyond it.

### It cannot see a purely passive interceptor

Some surveillance only listens. A device that receives the unencrypted
broadcasts and paging every tower transmits, and never itself transmits
anything, gives Rayhunter nothing to notice, there is no fake tower for your
phone to connect to, no request aimed at your device, no artifact in your
traffic. The techniques Rayhunter catches all involve a device *doing*
something to a phone. One that merely watches the open channels stays outside
its reach entirely.

### Its coverage is uneven across network generations

Rayhunter's detectors were built mainly for 4G (LTE), where most of them look
for their patterns. Coverage of the other generations is partial, and worth
knowing precisely:

- **4G (LTE)** is where nearly every detector operates, and where the tool is
  strongest. Even here, one lower layer (the LTE MAC layer) is now written into
  the capture for inspection in other tools, but only one MAC message, the
  random access response, is read by a detector (the timing-advance check).
- **2G** is covered in one specific way in this fork, the
  [2G location detector](../detectors/rrlp.md), and 2G and 3G traffic is now
  recorded for later inspection, but there is no broad set of 2G/3G attack
  detectors. A great deal of what happens on those networks passes without a
  detector watching for it.
- **3G** signalling is recorded where seen but is not analysed by any detector.
- **5G** is largely outside the current detectors' view.

The practical consequence: Rayhunter is most likely to catch something on 4G,
and an attack conducted entirely on a generation it does not analyse can happen
without a warning. This is one more reason a
[downgrade to 2G](./attack-downgrade.md) matters, it can move a phone onto
ground the tool watches less closely.

### It does not analyse every layer even where it looks

Even on 4G, Rayhunter reads the signalling that sets up and steers a
connection, the control conversation, not every kind of message on the
network. Some traffic is recorded for inspection in other tools but is not
examined by any detector. The [detector reference](../detectors/index.md) is
the exact list of what is watched; anything not on it is not being checked,
however visible it may be in a recording.

### A quiet Rayhunter is not a clean bill of health

Putting the above together: silence means the detectors that are running,
watching the generations they watch, on the traffic this one device received,
found nothing they are built to flag. It does not mean no device was present,
that nothing happened on an unwatched layer or generation, or that a passive
listener was not recording the whole time. Absence of a warning is genuine
information, it is narrower information than it feels like at 2 a.m.

## The precise details

For readers going into the code or the [detector reference](../detectors/index.md):

- **The vantage point** is the modem's diagnostic stream: Rayhunter parses the
  RRC and NAS control messages its own radio logged, as described in
  [How Cell Networks Work](./cell-networks.md). It transmits nothing and
  injects nothing.
- **Generation coverage** reflects the analyzers that exist:
  `lib/src/analysis/` holds detectors written mainly against LTE RRC and NAS,
  plus the fork's RRLP detector for 2G positioning and its timing-advance
  detector, which reads one specific LTE MAC message (the random access
  response). Most 2G and 3G traffic is still deliberately collected and written
  to the recording for reading in tools like Wireshark without being passed to
  attack detectors, a point the code makes explicitly where it decides not to
  report such messages as parse failures.
- **What "passive" cuts both ways.** Rayhunter's own passivity is a safety and
  stealth feature (it cannot be easily detected, and running it transmits
  nothing). The same passivity is why it cannot see an attacker who is equally
  passive. [Why Detection Is Hard](./why-detection-is-hard.md) covers this
  trade-off; the research on what a passive listener can learn from open
  channels is in [Sources and Further Reading](../references.md).
- **Validation is a separate limit from coverage.** Even for what Rayhunter
  does watch, some detectors have been tested only against reference vectors,
  not real traffic, a caveat carried on each detector page and in
  [How We Validate Detectors](../detectors/validation.md). A detector that
  watches for something can still be wrong about it.

## Where to next

[Legal and Personal Risk](./risk.md) turns from what the tool can know to what
you should weigh when you act on what it tells you.
