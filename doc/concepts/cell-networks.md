# How Cell Networks Work

Your phone is in your pocket while you walk across town. You take no action.
You may not look at it once. In those twenty minutes it holds a quiet,
constant conversation with the network: it listens to nearby equipment
announcing itself, changes which antenna it talks to several times, and tells
the network roughly where it is, so that a call can find you the moment
someone dials.

This page explains that hidden conversation. Every attack described later in
this book is a twist on some ordinary step of it, so the ten minutes this
page takes is what makes the rest of the book readable. There are no
instructions here — nothing to do, one thing to understand.

## What you would see

Almost nothing, and that is the point. The visible surface of a cell network
is a few bars of signal, a carrier's name, a small label like "4G" or "5G",
and calls and messages that arrive. When you travel, the bars dip and recover.
Everything else is designed to be invisible, and a phone doing surveillance
protocol work looks identical to a phone doing nothing.

## Why it matters

Two facts follow from how this system works, and they frame everything else
in this book.

First, **knowing roughly where you are is not an attack — it is how the
network functions.** A call can only reach you because the network keeps
track of the area your phone is in. The machinery for finding a phone is
built in, everywhere, all the time. The attacks this book cares about do not
break into that machinery. They operate it, while pretending to be a
legitimate part of it.

Second, **your phone extends a lot of trust before any proof is offered.**
Early in every connection there is a stretch where your phone and the network
talk in the open, before the network has proven who it is and before
encryption switches on. Nearly every attack in this book lives in that
stretch. EFF's 2019 white paper, *Gotta Catch 'Em All*
([Sources and Further Reading](../references.md)), walks through this
window in detail; what follows is the short version.

## How it works

### Towers and cells

The network divides the world into patches of coverage. Each patch is served
by one piece of carrier equipment — the box of radios and antennas people
point to and call a *tower* (the technical term is *base station*). Each
patch is called a *cell*, which is where "cell phone" comes from. Your phone
talks to one tower at a time, and the towers are wired back into the
carrier's core network, which does the routing, the billing, and the
record-keeping.

A tower is not one fixed thing. It can be a mast on a hill serving kilometers
around it, a box on a rooftop serving a block, or — and this matters later —
a device the size of a briefcase that someone carried in.

### Broadcasts: how your phone finds a network

Every tower continuously announces itself: which carrier it belongs to, what
its neighbours are, and the ground rules for connecting to it. Your phone
scans these announcements (the *broadcast messages*) to decide where to
connect, the way you might scan shop signs on an unfamiliar street.

Two properties of these announcements matter. They are sent in the open, with
no encryption. And they carry no signature — nothing in a broadcast proves it
came from the carrier it names. Your phone believes them on sight, and so
does every phone around it. Anyone with a radio can listen to them, and
anyone with the right equipment can transmit their own
([*Gotta Catch 'Em All*](../references.md)).

### Attaching: the joining ceremony

When your phone picks a tower and joins the network, the two sides run
through a short ceremony called an *attach*. Stripped down, it goes like
this:

1. Your phone asks to connect, and says who it is.
2. If the network cannot work out who is asking, it can send an *identity
   request* — "tell me exactly who you are."
3. The network proves itself: using secrets stored in your SIM card, it
   demonstrates that it is really your carrier, and your phone checks the
   proof. This step is called *authentication*.
4. Both sides switch on encryption.
5. The network accepts the phone, and hands it a fresh temporary name — the
   next section explains those.

The order is the whole story. Steps 1 and 2 happen **before** the proof in
step 3 and the encryption in step 4. A phone will answer an identity request
from a tower that has proven nothing, because sometimes a legitimate network
genuinely needs to ask. The rules require so little at that stage that the
window is real, and it is the single most abused moment in the whole
protocol: the [identity capture attack](./attack-identity.md) lives entirely
inside it.

<!-- DIAGRAM: message sequence, two lifelines.
Elements: "Your phone" (left), "Tower" (right).
Messages, top to bottom:
  1. phone -> tower:  "Request to connect (includes a name)"  — label: Attach request
  2. tower -> phone:  "Who are you, exactly?"                 — label: Identity request (optional step)
  3. phone -> tower:  "My permanent number is..."             — label: Identity response
  4. tower -> phone:  "Proof I know your SIM's secret"        — label: Authentication (the proof step)
  5. phone -> tower:  "Proof checks out; here is mine"
  6. both, shaded band from here down:                        — label: Encryption switches on
  7. tower -> phone:  "Accepted. Your temporary name is..."   — label: Attach accept
A bracket spans messages 1–3, labeled: "Open and unproven: anyone can ask, and the phone will answer."
Caption: "Joining a network. Everything above the proof step travels in the
open and requires no evidence the tower is genuine. Most of the attacks in
this book operate in that stretch."
-->

### Your phone's three names

Three identifiers stand for you on the network, and telling them apart is
half the vocabulary of this book.

- **The permanent number that identifies your SIM card (the IMSI).** One per
  SIM, globally unique, and it does not change. Whoever collects it can
  recognise you at any later time, anywhere in the world. This is the number
  surveillance equipment exists to harvest, and the reason those devices are
  called *IMSI catchers*.
- **The permanent number that identifies the phone hardware (the IMEI).**
  It survives a SIM swap, which is exactly why it interests the same people.
- **A temporary name (the TMSI).** Once your phone has attached, the network
  assigns it a short-lived alias and uses that in place of the IMSI wherever
  possible. It changes from time to time and means nothing once retired.

The temporary name is a privacy measure, and mostly it works: day to day,
what travels over the air is the alias. Which is why so much attacker effort
goes into manufacturing the exceptional moments when the permanent number is
sent instead.

### Paging: how a call finds you

An idle phone stays almost silent to save power. So when a call or message
arrives, the network calls out for it: towers in your area transmit a *page*
— "the phone with this name, come and get your call" — on a channel every
phone in the area listens to.

Ordinarily the page uses your temporary name. Sometimes it uses the
permanent one; the standards allow it. And the paging channel, like the
broadcasts, is unencrypted — anyone nearby can watch pages go out
([*Gotta Catch 'Em All*](../references.md) covers what can be learned this
way). Remember both properties when you reach the pages on locating attacks.

### Moving: handover and reselection

Walk far enough and you leave one tower's cell for another's. Nothing visible
happens, but underneath, one of two things occurred. An idle phone re-decides
for itself, using the priorities carried in the broadcasts — this is
*reselection*. A phone mid-call is told to move by the network — this is
*handover* — based on signal measurements the phone reports back to its
tower.

Both mechanisms take direction from the network side, and both can be
steered: broadcast priorities can herd idle phones toward a particular
network, and the measurement reports phones send can be unencrypted and
detailed enough to work out where the phone is
([*Gotta Catch 'Em All*](../references.md)). The
[downgrade](./attack-downgrade.md) and
[location](./attack-location.md) pages pick these threads up.

### Generations

Networks of several generations run at once — 2G, 3G, 4G, 5G — and your
phone moves between them as coverage demands. The label is not only about
speed. Each generation also fixed security problems in the one before it.

The gap that matters most here: on 2G, designed in the late 1980s and early
1990s, the proof step of the attach ceremony runs one way. The phone proves
itself; the network proves nothing. A 2G phone cannot tell a genuine tower
from an impostor, and 2G's encryption is weak enough to be broken outright
([*Gotta Catch 'Em All*](../references.md)). Later generations added mutual
proof and stronger encryption — a 4G tower must pass step 3 or the phone
walks away.

Phones keep the old generations for coverage, and fall back to them when
told nothing better is available. Being deliberately pushed down to an old
generation is therefore an attack with a name, and a
[page of its own](./attack-downgrade.md).

## The precise details

For readers heading into the [detector reference](../detectors/index.md) or
the code, this maps the plain words above to protocol terms. The
[Glossary](../glossary.md) collects all of them.

- **Tower.** In LTE (4G), the base station is an *eNodeB*. The messages your
  phone exchanges with it directly are *RRC* (Radio Resource Control,
  3GPP TS 36.331); the conversation with the core network behind it —
  attach, identity, authentication — is *NAS* (Non-Access Stratum,
  3GPP TS 24.301). Rayhunter's detectors read both streams, and these two
  names appear throughout the [packet explorer](../packet-explorer.md).
- **Broadcasts.** *System Information Blocks* (SIBs), numbered SIB1, SIB2,
  and so on, each carrying a defined slice of the announcement — SIB6 and
  SIB7, for instance, carry the reselection priorities for 3G and 2G
  neighbours. Carried on the BCCH channel.
- **The attach ceremony.** NAS *Attach Request*, *Identity Request* /
  *Identity Response*, *Authentication Request* / *Authentication Response*,
  *Security Mode Command* (the encryption switch-on), *Attach Accept*.
  These exact names label messages in Rayhunter recordings, and the
  [identity detector](../detectors/imsi-requested.md) is a state machine over
  them.
- **The three names.** IMSI: International Mobile Subscriber Identity, on
  the SIM. IMEI: International Mobile Equipment Identity, in the hardware.
  TMSI: Temporary Mobile Subscriber Identity; LTE wraps it in a larger
  temporary identity called the GUTI, but this book says TMSI for the whole
  idea of the rotating alias.
- **Paging.** Carried on the PCCH channel; a page naming an IMSI rather
  than a TMSI is the exceptional case discussed above.
- **Where Rayhunter fits.** A Rayhunter device is an ordinary hotspot, on
  the network like any phone. Its modem chip exposes a diagnostic interface,
  and Rayhunter records the same RRC and NAS control messages this page
  described, as the modem saw them. It watches only what the network sends
  *to this device* — a limit explained in
  [What This Tool Cannot Tell You](./limitations.md).

## Where to next

With this vocabulary, the rest of the Understanding section reads in order:
[What a Cell-Site Simulator Is](./cell-site-simulators.md) introduces the
equipment that abuses these mechanisms, and
[How These Attacks Actually Work](./attacks.md) walks the four families of
abuse one at a time.
