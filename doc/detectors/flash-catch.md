# Identity Taken, Then Forged Authentication (FlashCatch)

Watches for a tower that fails your phone's own authentication check again and
again: the mark of a fake tower that took your identity and then made your
phone walk away from it.

This detector is an **addition in this fork** and is not present upstream. It
is built from the published description of the attack and the standard's
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
tower asked for the permanent identity while the phone was merely reporting
its location (a tracking area update). On its own that is not alarming.

## Why it matters

A conventional [cell-site simulator](../concepts/cell-site-simulators.md)
takes a phone's identity and then holds on to the phone, or rejects it in a
way that makes it start over. Either leaves a trace a person can feel: service
drops for a while. The attack this detector looks for was designed to leave
none.

FlashCatch, described by Andrea Paci, Gabriele Bologna, Ivan Palamà and
Giuseppe Bianchi at ACM WiSec 2025, works like this. A fake tower pretends to
belong to the phone's own network, on a frequency the network really uses.
When the phone checks in with it, the tower immediately asks for the permanent
identity, the IMSI, which the phone supplies because it may legitimately be
asked before authentication. Then, instead of holding the phone, the tower
sends three authentication challenges it has deliberately signed wrongly. The
phone rejects each one as forged, and after the third it treats the tower as
having failed authentication, bars that cell for a while, and goes back to the
real network with its existing keys intact. The whole exchange takes well
under a second, and because the phone never lost its place on the real
network, nothing visible happens.

What the attacker gains is the identity, fast, with the phone none the wiser.
What the attacker cannot avoid is leaving the rejections in the phone's own
log. Rayhunter reads that log.

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
- **A real network asking for the IMSI during a location update.** This
  happens when the network has lost the phone's record, after maintenance for
  example. It is noted, not warned about; only forged challenges afterwards
  turn it into a warning.

Two forged-challenge rejections in a row on a working SIM is not something a
real network produces. That is what makes the signal worth a Medium on its
own.

## How it works

The phone's modem writes every mobility-management message it exchanges with
the tower into its log, in the clear, whether or not the message was
integrity-protected on the air. The detector reads three of them.

- **IDENTITY REQUEST** (3GPP TS 24.301 §8.2.18) asking for the IMSI. The
  detector remembers where it saw one.
- **AUTHENTICATION FAILURE** (§8.2.5) from the phone, with cause 20, "MAC
  failure" (§9.9.3.9): the phone found the challenge's signature wrong. Cause
  21, "Synch failure", is counted separately and more leniently.
- Any message showing the check passed or the exchange ended well:
  AUTHENTICATION RESPONSE, SECURITY MODE COMMAND, or an ATTACH, TRACKING AREA
  UPDATE or SERVICE accept. These clear the count.

A new tracking area update, attach or service request starts a fresh exchange
and forgets what came before it, so a rejection in one connection is never
added to a rejection in the next.

## Precise behavior

- On the **second** AUTHENTICATION FAILURE with cause MAC failure within a
  window of 200 log records, one warning is raised: **High** if an IDENTITY
  REQUEST for the IMSI arrived within the same window, otherwise **Medium**.
  Further failures in the same run add nothing; one warning per run.
- On the **third** AUTHENTICATION FAILURE with cause Synch failure within the
  window, one **Medium** warning.
- An IDENTITY REQUEST for the IMSI arriving within the window after a
  TRACKING AREA UPDATE REQUEST produces an **Informational** note. A bare
  identity request produces nothing here; the [identity
  detector](./imsi-requested.md) covers it.
- Identity requests for anything other than the IMSI (the IMEI, say) are
  ignored by this detector.
- The window is counted in log records, not seconds. The attack is over in a
  fraction of a second, a few dozen records; the window is generous because
  the modem logs other traffic in between.

## Validation

**Synthetic only.** The detector is exercised by hand-built messages encoded
per TS 24.301 and by a demonstration scenario; the sequence it looks for is
taken from the paper's description of the attack and from the standard's
rules for how a phone handles authentication failure (TS 24.301 §5.4.2.6, TS
36.304 §5.3.1). No recording of a FlashCatch attack was available to test
against. If you have one, or can make one in a controlled setting, it would
settle the question; see [How We Validate Detectors](./validation.md).

## Configuration

Enabled by default. To turn it off, set in the device's config:

```toml
[analyzers]
flash_catch = false
```

Or use the switch on the settings page of the web interface.

## Sources

- A. Paci, G. Bologna, I. Palamà, G. Bianchi, *FlashCatch: Sub-Second IMSI
  Catching with No Service Disruption*, ACM WiSec 2025.
- 3GPP TS 24.301, *Non-Access-Stratum (NAS) protocol for Evolved Packet
  System*: §5.4.2.6 (authentication not accepted by the UE), §8.2.5, §8.2.18,
  §9.9.3.9.
- 3GPP TS 36.304, *User Equipment procedures in idle mode*: §5.3.1 (cell
  barring after authentication failure).
- [EFForg/rayhunter#462](https://github.com/EFForg/rayhunter/issues/462), the
  upstream request for this detector.
