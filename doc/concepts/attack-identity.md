# Capturing Your Identity

You are at a demonstration. So are three thousand other people, each with a
phone in their pocket. Somewhere in the crowd, or in a van parked nearby, is a
device the size of a briefcase pretending to be a tower. It does not need
to listen to anyone's calls. It only needs to ask each phone one question, "what is your permanent number?", and write down the answers. An hour later
its operator has a list: the unique, unchanging identifier of every phone that
was in the square.

This is the oldest and most common thing a cell-site simulator does, and the
one Rayhunter is best at catching. This page explains what is actually being
taken and how the trick works. For the exact rules Rayhunter uses to flag it,
see the detector page, [Identity Requested Without
Authentication](../detectors/imsi-requested.md).

## What you would see

On your phone, nothing, the invisibility described in
[What a Cell-Site Simulator Is](./cell-site-simulators.md) applies here in
full. The whole exchange takes a moment, your phone reconnects to a real tower
straight afterward, and nothing about your call or your screen changes.

In a Rayhunter recording, if the pattern is caught, you would see a warning
naming an identity request, the tool's clearest statement that your permanent
identity may have been taken. [Your First Warning](../first-warning.md) walks
through what that looks like.

## Why it matters

Recall from [How Cell Networks Work](./cell-networks.md) that you have two
kinds of name on the network. A **temporary name** that rotates and means
nothing once retired, which is what your phone uses day to day. And a
**permanent number that identifies your SIM card**, one per SIM, globally
unique, unchanging. The whole point of the temporary name is to keep the
permanent one off the air, so that no one listening can follow you from one
session to the next.

The permanent number defeats that protection completely. Because it never
changes, anyone who records it once can recognise you any time they see it
again, anywhere in the world. Collected at a place and time, it becomes a
receipt that a specific person was there. Collected in several places over
weeks, the receipts join into a trail. This is why the equipment is named for
that number, the *IMSI catcher* exists to catch exactly this
([Sources and Further Reading](../references.md), EFF's white paper). Turning
your temporary name back into your permanent one is the single most
valuable thing a device in identification mode can do, because everything else
someone might want to know about your movements can be built on top of it.

## How it works

### The one moment the permanent number is sent

Your phone guards its permanent number. It sends the temporary name wherever
it can, and only falls back to the permanent one when it genuinely has to, most often the very first time it talks to a network that has no temporary
name on file for it yet, such as after being switched off for a long while.

A real network, in that situation, asks. The mechanism is a legitimate
message from [How Cell Networks Work](./cell-networks.md): the *identity
request*, "tell me exactly who you are." Your phone answers with its permanent
number, because sometimes that request is real and refusing it would mean no
service. The feature has to exist. The problem is who else can use it.

### Identify, then reject

A cell-site simulator in identification mode uses that exact feature, and adds
a tell. The sequence, drawn from Dabrowski et al.'s description of
identification mode ([Sources and Further Reading](../references.md)), goes:

1. The fake tower advertises itself attractively, and your phone connects, for the reasons in [What a Cell-Site Simulator Is](./cell-site-simulators.md).
2. It sends an identity request, and your phone answers with its permanent
   number.
3. It never authenticates. Recall that a real 4G network must prove itself
   with your SIM's secret before the conversation goes any further. The fake
   tower cannot, it does not hold that secret, so it skips the step.
4. Having got what it wanted, it rejects the connection or drops it. Your
   phone, pushed away, finds a real tower and reconnects.

Step 3 is the heart of it, and the heart of how Rayhunter tells this apart
from an ordinary identity request. A genuine network asks for your identity
and *then proves who it is*. A catcher asks for your identity and then reveals
it never could prove anything, by disconnecting you instead of continuing. It
is the request for your permanent name, followed by the absence of any proof
in return, followed by a disconnect (the whole shape, not any single message) that marks the pattern as suspicious. That progression is the model page
`heuristics.ts` in the repo holds up as the standard, and it is what the
[identity detector](../detectors/imsi-requested.md) is built around.

### Why this leaves a catchable signature

Most of what a cell-site simulator does is hard to distinguish from a
misconfigured but honest network. This technique is a partial exception,
because the identify-then-reject sequence is not something a working network
has much reason to do. That relative clarity is why identity capture is the
detection Rayhunter can make most confidently, though "most confidently"
still is not "certainly," which is the subject of the next two pages.

## The precise details

- **The permanent number** is the IMSI (International Mobile Subscriber
  Identity) on your SIM; the related permanent hardware number is the IMEI,
  and a catcher may ask for either. The **temporary name** is the TMSI,
  carried inside LTE's larger GUTI. All are defined in the
  [Glossary](../glossary.md) and introduced in
  [How Cell Networks Work](./cell-networks.md).
- **The messages.** The request is a NAS *Identity Request*; the proof step
  the fake tower skips is *Authentication* followed by the *Security Mode
  Command*. Rayhunter's detector is a state machine over these NAS messages,
  and it distinguishes several cases: an identity request after authentication
  has already succeeded, an identity request with no attach request preceding
  it, and a disconnect following an identity request with no authentication, weighing the last of these differently depending on whether the tower
  appears to be your home network or a roaming partner.
- **The known harmless case.** The same detector is documented to fire
  sometimes on aircraft coming in to land, where a phone that has been out of
  contact reconnects through towers that cannot reach its home network. This
  is not a footnote, it is exactly the kind of benign event that produces a
  real warning, and the detector page
  [Identity Requested Without Authentication](../detectors/imsi-requested.md)
  states it plainly. How to weigh a warning like that is the job of
  [Reading Warnings Without Panicking](./interpreting-warnings.md).
- **What Rayhunter cannot see here.** It sees an identity request aimed at
  *this* device. A catcher that never targets the Rayhunter device, or that
  reads identities without the reject step, may leave nothing for it to catch, see [What This Tool Cannot Tell You](./limitations.md).

## Where to next

[Downgrading You to Weaker Networks](./attack-downgrade.md) is often the next
move after an identity is taken, and the setup for everything worse.
