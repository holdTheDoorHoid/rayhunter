# Identity Taken, Then Forged Authentication (FlashCatch)

Watches for a tower that fails your phone's own authentication check again and
again: the mark of a fake tower that took your identity and then made your
phone walk away from it.

This detector is an **addition in this fork** and is not present upstream. It
is built from the paper that describes the attack and from the standard's
rules for authentication failure; it has **not** been confirmed against a
recording of the attack itself. See [validation](#validation) below.

## What you would see

A High warning saying the tower asked for the phone's permanent identity and
then failed its own authentication two or more times in a row, naming the
packets. Or a Medium warning for the repeated failures alone, when no identity
request came just before. Either way the phone itself will have shown nothing:
no dropped call, no lost signal, no moment without service. That is the point
of the attack.

An Informational note, visible only beside another warning, marks the moment a
tower asked for the permanent identity as soon as the phone checked in with it
(a tracking area update or a service request). On its own that is not
alarming.

A tower that takes the identity again later raises a fresh warning each time,
so a device that stays near one collects a warning per capture.

## Why it matters

A conventional [cell-site simulator](../concepts/cell-site-simulators.md)
takes a phone's identity and then holds on to the phone, or rejects it in a
way that makes it start over. Either leaves a trace a person can feel: service
drops for a while. The attack this detector looks for was designed to leave
none.

FlashCatch, described by Andrea Paci, Gabriele Bologna, Ivan Palamà and
Giuseppe Bianchi at ACM WiSec 2025, works like this. A fake tower pretends to
belong to the phone's own network, on a frequency that network really uses, so
the phone drifts onto it as it would onto any stronger cell. The phone checks
in: with a tracking area update if the fake tower advertises a different
tracking area than the real one, or with a service request if it copies it.
The tower immediately asks for the permanent identity, the IMSI, which the
phone supplies because the standard lets that request arrive before any
security is set up. Then, instead of holding the phone, the tower sends three
authentication challenges it has deliberately signed wrongly. The phone
rejects each one as forged, and after the third it treats the tower as having
failed authentication, bars that cell for five minutes, and goes back to the
real network with its existing keys and temporary identity intact.

The paper's measurements say how little there is to notice. On seven phones
from three baseband makers the identity left the phone within 40 ms of the
check-in; in a field test with fifty volunteers the average was 27 ms. The
only effect the authors could measure was a latency spike of about a second,
even during a voice call. And because the phone keeps its temporary identity
(the GUTI), whoever holds the IMSI can follow that temporary identity
afterwards without asking again; the phone does not return to the fake cell
for at least five minutes, but the authors note that changing the cell's
physical identity gets around the bar, so the identity can be taken again at
will.

What the attacker gains is the identity, fast, with the phone none the wiser.
What the attacker cannot avoid is leaving the rejections in the phone's own
log. The authors say so themselves: the triple authentication failure is the
attack's one distinctive trace, and the phone-based detectors they tried it
against (AIMSICD, Cell Spy Catcher and CellGuard) did not flag it. Rayhunter
reads that log.

## When it fires harmlessly

Rarely, and the cases are recognisable.

- **A SIM whose key does not match the network's.** A badly provisioned SIM,
  or a test SIM used on the wrong network, fails authentication on every
  attempt and would trip this detector every time the phone connects. Such a
  phone also has no service at all, so the situation is not subtle.
- **Network trouble during a key change.** Very occasionally a real network
  has one failure while it and the phone resynchronise. One failure does not
  count, and a sequence-number mismatch (a different, gentler cause) needs
  three in a row before this detector says anything.
- **A real network asking for the IMSI when the phone checks in.** This
  happens when the network has lost the phone's record, after maintenance for
  example. It is noted, not warned about; only forged challenges afterwards
  turn it into a warning.

Two forged-challenge rejections in a row on a working SIM is not something a
real network produces. That is what makes the signal worth a Medium on its
own.

## How it works

The phone's modem writes every mobility-management message it exchanges with
the tower into its log, in the clear, whether or not the message was
integrity-protected on the air. The detector reads three kinds of them.

- **IDENTITY REQUEST** (3GPP TS 24.301 §8.2.18) asking for the IMSI. The
  detector remembers where it saw one.
- **AUTHENTICATION FAILURE** (§8.2.5) from the phone, with cause 20, "MAC
  failure", or cause 26, "non-EPS authentication unacceptable" (§9.9.3.9).
  Both mean the phone found the challenge unacceptable without needing the
  key to be right, and the standard counts both towards barring the cell, so
  the detector counts them together as forged challenges. Cause 21, "Synch
  failure", needs a genuine challenge replayed and is counted separately and
  more leniently.
- Any message showing the check passed or the exchange ended well:
  AUTHENTICATION RESPONSE, SECURITY MODE COMMAND, or an ATTACH, TRACKING AREA
  UPDATE or SERVICE accept. These clear the count.

A new check-in (a TRACKING AREA UPDATE REQUEST, EXTENDED SERVICE REQUEST or
CONTROL PLANE SERVICE REQUEST) or an ATTACH REQUEST starts a fresh exchange
and forgets what came before it, so a rejection in one connection is never
added to a rejection in the next. The short SERVICE REQUEST message has no
plain form for the parser to decode, so an identity request after one of
those is not noted; the warnings do not depend on it.

## Precise behavior

- On the **second** AUTHENTICATION FAILURE with cause MAC failure or non-EPS
  authentication unacceptable within a window of 200 log records, one
  warning is raised: **High** if an IDENTITY REQUEST for the IMSI arrived
  within the same window, otherwise **Medium**. Further failures in the same
  run add nothing; one warning per run. The standard lets the phone bar the
  cell after three, so the warning comes one rejection before the phone
  leaves.
- On the **third** AUTHENTICATION FAILURE with cause Synch failure within the
  window, one **Medium** warning.
- An IDENTITY REQUEST for the IMSI arriving within the window after a
  TRACKING AREA UPDATE REQUEST, EXTENDED SERVICE REQUEST or CONTROL PLANE
  SERVICE REQUEST produces an **Informational** note. A bare identity request
  produces nothing here; the [identity detector](./imsi-requested.md) covers
  it.
- Identity requests for anything other than the IMSI (the IMEI, say) are
  ignored by this detector.
- The window is counted in log records, not seconds. The attack is over in a
  fraction of a second, a few dozen records; the window is generous because
  the modem logs other traffic in between.

## Validation

**Synthetic only.** The detector is exercised by hand-built messages encoded
per TS 24.301 and by a demonstration scenario; the sequence it looks for is
taken from the paper's account of the exchange (its Figure 3 and Sections 4.1
to 4.4) and from the standard's rules for how a phone handles authentication
failure (TS 24.301 §5.4.2.6 and §5.4.2.7, TS 36.304 §5.3.1). The paper does
not come with a recording: its field data were processed live and discarded.
No recording of a FlashCatch attack was available to test against. If you
have one, or can make one in a controlled setting, it would settle the
question; see [How We Validate Detectors](./validation.md).

## Configuration

Enabled by default. To turn it off, set in the device's config:

```toml
[analyzers]
flash_catch = false
```

Or use the switch on the settings page of the web interface.

## Sources

- A. Paci, G. Bologna, I. Palamà, G. Bianchi, *FlashCatch: Minimizing
  Disruption in IMSI Catcher Operations*, ACM WiSec 2025, Arlington, VA,
  [doi:10.1145/3734477.3734705](https://doi.org/10.1145/3734477.3734705).
  Sections 4.1 to 4.4 describe the exchange, 5.2 and 5.4 the timings, 5.5 the
  retained temporary identity, and 6 the attack's own detectability.
- 3GPP TS 24.301, *Non-Access-Stratum (NAS) protocol for Evolved Packet
  System*: §4.4.4.2 (identity and authentication requests are accepted
  without integrity protection, the clause the attack exploits), §5.4.2.6
  (authentication not accepted by the UE), §5.4.2.7 (after three consecutive
  failures the UE treats the cell as barred), §8.2.5, §8.2.18, §9.9.3.9.
- 3GPP TS 36.304, *User Equipment procedures in idle mode*: §5.3.1 (a cell
  treated as barred is avoided for 300 seconds).
- [EFForg/rayhunter#462](https://github.com/EFForg/rayhunter/issues/462), the
  upstream request for this detector.
