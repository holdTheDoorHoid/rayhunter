# Making Your Phone Report Its Position

Every technique so far has been about someone working out something *about*
you — who you are, what network you are on, what you are saying. This one is
different, and more precise. It does not work your position out. It asks your
phone to measure exactly where it is and send the answer back. Your phone
carries a satellite receiver and can time signals from towers to within
fractions of a second; this technique turns that capability around and points
it at you.

The feature it uses is real and, in its intended form, benevolent: it is how
an emergency call is located, so that help can be sent to the right place.
The same feature, aimed differently, is the most exact locating a network can
do. This page explains how a phone comes to report its own position, and the
difference between being located once and being followed. It maps to the
detectors [Location Requested (LPP)](../detectors/lpp.md) and
[Location Requested on 2G (RRLP)](../detectors/rrlp.md).

**These location detectors are an addition in this fork of Rayhunter.** They
watch for a category of request the upstream project does not yet flag. Two
honest notes come with that, stated here and in full on the detector pages:
the detectors were verified against reference encodings of the protocol, not
yet against a recording of a real network making one of these requests; and
because they read a network feature that has ordinary uses, a warning here
needs the same careful reading as any other.

## What you would see

Nothing on the phone. There is no indicator when your position is measured for
the network, the way some phones show a brief icon when an app uses location.
This request goes to the modem, beneath the part of the phone you interact
with, and produces no sign on the screen.

In a Rayhunter recording, a caught request appears as a warning that your
location was asked for, and — if the deeper detector is running — whether it
was asked for once or on a repeating timer. That distinction is the most
important thing on the page, and the next section is why.

## Why it matters

The other techniques leak your location only roughly, as a side effect. A
tower knows which cell you are in, which narrows you to an area. Researchers
have shown a semi-passive attacker can place a phone within roughly a
two-square-kilometre area in a city, and that the measurement reports phones
send out can be detailed enough to trilaterate a position, sometimes down to
exact GPS coordinates (Shaik et al., 2016; EFF's white paper on measurement
reports — [Sources and Further Reading](../references.md)).

This technique skips the estimation. It asks your phone for a fix, and your
phone can answer with satellite-grade coordinates. That is a difference in
kind, not degree.

And it can ask for that fix **repeatedly**. A single request during an
emergency call is ordinary. A standing request that reports your position
every few seconds, for as long as it lasts, is not a locate — it is a track.
The gap between those two is the whole subject of surveillance, and reading
which one you are looking at is what the deeper of the two location detectors
exists to do.

## How it works

### A protocol for asking a phone where it is

Mobile networks include a dedicated protocol for positioning. The network
sends a request — measure your position and report back — and your phone
complies, because the feature was built for a good reason and the phone cannot
tell a worthy request from an unworthy one. On modern networks this protocol
is called LPP; on 2G there is an older equivalent called RRLP. They do the
same job in different eras, which is why Rayhunter has a detector for each.

The request can name *how* it wants your phone to find itself, and the choice
says a lot about the intent and the precision:

- **By satellite** — your phone reads GPS or other satellite systems, with
  help from the network to do it faster. This is the most precise, a true fix.
- **By tower timing** — your phone measures the tiny differences in when
  signals arrive from several towers, which pins it down by geometry. Precise,
  and it makes your phone actively measure.
- **By cell** — which cell you are in, plus timing to it. The coarsest, but
  cheap and quiet.

Which method a request asks for is part of what the deeper detector reads and
reports, because a demand for a satellite fix is a different thing from noting
which cell you are near.

### Once, or over and over

The single most telling field in one of these requests is whether it asks for
a *one-off* report or a *periodic* one. A one-off request produces a single
answer. A periodic request sets up continuous reporting: your phone sends its
position again and again, on a timer, until the session is ended.

Periodic reporting is the signature that separates a routine locate from
tracking, and Rayhunter treats it accordingly. The basic detector notes that a
location exchange happened at all. The deeper detector reads far enough into
the request to tell one-off from periodic, and raises the periodic case to a
higher severity than the single one — because "the network asked where I am"
and "the network arranged to be told where I am, continuously" are genuinely
different events. The [detector page](../detectors/lpp.md) gives the exact
severities and rules.

### Why it is split across two networks

Because the modern protocol (LPP) and the 2G one (RRLP) are separate, and
because being [pushed onto 2G](./attack-downgrade.md) is itself a known move,
the two detectors matter together. A downgrade to 2G followed by a 2G location
request is a more coherent story than either event alone. The 2G location
detector exists so that story does not go unnoticed in the places, and the
moments, where a phone is on 2G.

## The precise details

- **The protocols.** LPP is the LTE Positioning Protocol, 3GPP TS 36.355;
  RRLP is the Radio Resource LCS Protocol, 3GPP TS 44.031, carried inside a
  GSM Radio Resource message on 2G ([Sources and Further Reading](../references.md)).
- **The methods**, in the protocol's terms: A-GNSS (assisted satellite
  positioning), OTDOA (Observed Time Difference Of Arrival — the tower-timing
  method), and E-CID (Enhanced Cell ID — cell plus timing). These names appear
  in the [detector page](../detectors/lpp.md) and the
  [Glossary](../glossary.md).
- **The three detectors.** A basic LPP detector notes any location exchange; a
  deeper LPP detector decodes the method and the one-off/periodic distinction
  and can be turned off separately on devices very short on memory; and the
  RRLP detector covers the 2G case. Sources: `lib/src/analysis/lpp.rs` and
  `lib/src/analysis/rrlp.rs`.
- **Validation, stated plainly.** The LPP layouts were verified against
  pycrate's reference TS 36.355 encoder — a check that caught a real one-bit
  error during development — and the RRLP layouts against reference TS 44.018
  and TS 44.031 encoders. Neither has yet been confirmed against a capture of
  a real network issuing one of these requests; the fork's test devices see
  only LTE, so the 2G detector in particular is unexercised against live
  traffic. [How We Validate Detectors](../detectors/validation.md) explains
  what that level of evidence is and is not worth.
- **The honest uses.** Emergency calls use this protocol to be located, and
  some carriers use it for lawful location-based services. A hotspot sitting
  on a desk should see it rarely, so an unexplained warning is worth
  attention — but "worth attention" is not "proof," and
  [Reading Warnings Without Panicking](./interpreting-warnings.md) is the
  page for turning one into the other.
- **What Rayhunter cannot see.** It sees a positioning request aimed at this
  device. Location worked out passively from measurement reports, without any
  explicit request — the Shaik and white-paper mechanism above — is a
  different exposure that this detector does not cover; see
  [What This Tool Cannot Tell You](./limitations.md).

## Where to next

That completes the four techniques. [Why Detection Is
Hard](./why-detection-is-hard.md) steps back to explain why none of them can
be caught with certainty, and [Reading Warnings Without
Panicking](./interpreting-warnings.md) is the page to read before you act on
anything Rayhunter tells you.
