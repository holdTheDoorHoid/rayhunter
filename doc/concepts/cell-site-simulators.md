# What a Cell-Site Simulator Is

Imagine someone sets up a stall in a crowded square and hangs a sign reading
"Post Office." It is not the post office. But it looks close enough that
people walk up and hand over their letters, and the person behind the stall
writes down every name and return address before quietly passing the letters
along to the real post office down the street. Nobody's mail is delayed.
Nobody notices. And at the end of the day the stall owner has a list of
everyone who was in the square.

A cell-site simulator is that stall, for phones. It is a device that pretends
to be a tower so that phones nearby will connect to it and reveal who
they are. This page is about what that device is, who has them, and why a
phone falls for it. It assumes you have read
[How Cell Networks Work](./cell-networks.md), because the trick depends
entirely on the ordinary machinery described there.

## What you would see

Nothing. That is worth sitting with for a moment, because it shapes what a
tool like Rayhunter can and cannot do for you.

When your phone connects to a cell-site simulator, your phone does not warn
you. Your calls still connect. Your messages still arrive. The bars still
show signal. From where you stand, being caught by one of these devices and
walking past an ordinary tower look exactly the same. There is no pop-up, no
sound, no sign on the screen.

This is not a flaw someone forgot to fix. The device is designed to be
invisible, and the parts of the network it abuses were designed to work
without bothering you. So the honest starting point is this: you cannot feel
one of these, and neither can anyone else without special equipment. That is
the entire reason detection tools exist.

## Why it matters

These devices are not hypothetical, and they are not only a concern for
spies. They are a standard piece of law-enforcement equipment, sold by
several companies to police departments and government agencies around the
world. The best-known brand name, *Stingray*, has become a generic word for
the whole category the way "Xerox" once stood in for any photocopier. EFF's
Street-Level Surveillance project, which is written for journalists, activists
and defense lawyers rather than engineers, is the clearest short account of
how they are actually used ([Sources and Further Reading](../references.md)).

What a person stands to lose depends on which of the device's capabilities is
being used, and the range is wide:

- **At the low end, your presence.** The simplest use collects the permanent
  identifiers of every phone in range. Run at a protest, a place of worship,
  or a border crossing, that produces a list of who was there, enough to
  place a specific person at a specific place and time
  ([EFF's protest surveillance guide](../references.md) walks through this
  scenario).
- **At the high end, your communications.** More capable devices can push
  your phone onto a weak network and read or alter what passes through, the
  [downgrade](./attack-downgrade.md) and [encryption](./attack-encryption.md)
  pages cover how.

You do not get to choose which capability is pointed at you, and you cannot
tell from the outside which one it was. That uncertainty is part of why
[reading a warning honestly](./interpreting-warnings.md) takes care: the tool
sees the technique, not the intent behind it.

## How it works

### Why your phone believes the sign

Recall two facts from [How Cell Networks Work](./cell-networks.md). Towers
announce themselves in the open, and nothing in those announcements proves
they came from a real carrier. And your phone is built to prefer the tower
with the strongest, most convenient signal, because normally that is the one
nearest you.

A cell-site simulator exploits both. It transmits the same kind of
announcements a real tower does, claiming to belong to your carrier, often
with a strong signal and attractive settings so that nearby phones prefer it.
The phones have no way to check the claim before they act on it, so they
connect, exactly as they were designed to. The white paper *Gotta Catch 'Em
All* ([Sources and Further Reading](../references.md)) lays out this
mechanism in full; the short version is that the phone is not tricked by
anything clever. It is behaving correctly, in a system that asks it to trust
first and verify later.

### The two things it can do once you connect

A 2014 research paper by Dabrowski and colleagues
([Sources and Further Reading](../references.md)) drew a distinction that is
still the most useful way to think about these devices. Once your phone has
connected, the operator is doing one of two things.

**Identification.** The device lures your phone in, asks it for its permanent
identity, gets the answer, and then pushes the phone back to its real network
by rejecting the connection. The whole interaction lasts a moment. Your phone
reconnects to a genuine tower and carries on, and the operator has what they
came for: your identity, and proof you were in range. This is the mode built
for harvesting a crowd. Rayhunter's
[identity detector](../detectors/imsi-requested.md) is written to catch this
exact sequence, an identity demanded, no proof of the network offered, then
a disconnect.

**Camping.** The device keeps your phone connected and sits in the middle,
between you and the real network, passing your traffic along so that
everything keeps working while it watches. In this position it can attempt
the deeper attacks: forcing weak or absent encryption, seeing who you
contact. It is more powerful and more effort, and it holds your phone rather
than releasing it.

<!-- DIAGRAM: spatial, man-in-the-middle position.
Three boxes left to right: "Your phone", "Cell-site simulator (fake tower)", "Real carrier network".
Top row (Identification mode):
  phone -> fake:  "Who are you?" then fake -> phone: "Rejected, go away"
  fake is NOT connected onward to the real network (show the link to "Real carrier network" as absent / greyed).
  annotation: "Grabs your identity, then pushes you back to a real tower. Over in a moment."
Bottom row (Camping mode):
  phone <-> fake <-> real, all links solid, arrows both directions through the fake tower.
  annotation: "Keeps you connected and relays your traffic, watching from the middle."
Caption: "The two ways a cell-site simulator is used. In identification mode
it only wants your identity and lets you go; in camping mode it holds your
phone and sits between you and the real network. Rayhunter's detectors are
tuned mostly to the first, which leaves a cleaner signature."
-->

### Why they became common

The same Dabrowski paper records the shift that made these a widespread
concern rather than a government-only one. As more vendors entered the market
and prices fell, it became possible to build a working device from open-source
software and general-purpose radio hardware for roughly US$1,500
([Sources and Further Reading](../references.md)). A capability that once
required a government budget moved within reach of a much larger set of
people. That is the reason a cheap, passive detector you can carry is worth
building: the threat is no longer rare enough to ignore.

## The precise details

For readers going deeper, and to connect this page to the protocol vocabulary
in the [Glossary](../glossary.md):

- **Names.** "IMSI catcher", "cell-site simulator", "CSS", and "Stingray" all
  refer to the same category of device. "Stingray" is one manufacturer's
  product name (Harris Corporation) used generically. This book says
  *cell-site simulator* for the device and *IMSI catcher* where the emphasis
  is on identity capture, and treats them as the same thing.
- **Identification vs camping** are Dabrowski et al.'s terms (ACSAC 2014),
  and their taxonomy of operating modes maps onto Rayhunter's detectors, the
  [identity detector](../detectors/imsi-requested.md) targets the
  identification pattern specifically. See
  [How We Validate Detectors](../detectors/validation.md) for what "targets"
  is worth in evidence.
- **What "pretends to be a tower" means concretely.** The device transmits
  the broadcast channels (the SIBs of [How Cell Networks Work](./cell-networks.md))
  for a cell that appears to belong to a real carrier, chosen to be an
  attractive reselection or handover target. Later generations (4G, 5G) made
  this harder by requiring the network to authenticate, so a common move is
  to first push the phone down to a generation that does not, which is the
  [downgrade attack](./attack-downgrade.md).
- **The limit of a passive detector.** Rayhunter is on the network as an
  ordinary device and sees only what a tower sends *to it*. It can see the
  signature of a device operating on it; it cannot see a device that is
  purely listening, nor one that never targets the Rayhunter device itself.
  [What This Tool Cannot Tell You](./limitations.md) is the honest boundary.

## Where to next

[How These Attacks Actually Work](./attacks.md) takes the two modes described
here and breaks them into the four specific techniques Rayhunter watches for,
each with its own page and its own detector.
