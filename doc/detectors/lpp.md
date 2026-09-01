# Location Requested (LPP)

Watches for the network asking your device to measure and report exactly where
it is, and, in more depth, for whether it asked once or asked to be told
continuously.

This page covers two related detectors that read the same messages to different
depths: `lpp_location_request`, the basic check, and `lpp_location_tracking`,
the deeper one that reads far enough to tell a single locate from continuous
tracking. Both are **additions in this fork** and are not present upstream.

## What you would see

From the basic detector, a Low warning that the network asked your device for
its location, or that your device reported it. From the deeper detector, a Low
warning for a one-off location request and a **Medium** warning when the network
asked for *continuous* tracking, position reported over and over on a timer.
Both appear among a recording's warnings; the routine technical chatter around a
location exchange (the network asking what your device can measure, GPS
assistance data) is recorded quietly as informational notes rather than
warnings.

## Why it matters

Every other detector infers your location roughly, as a side effect. This one
is about the network asking your device to *state* it. As [Making Your Phone
Report Its Position](../concepts/attack-location.md) explains, the positioning
protocol exists for good reasons, locating an emergency call, but the same
machinery lets whoever controls the network ask your device for a
satellite-grade fix, and ask for it to repeat. A warning here means your
position was requested by name, which is a more precise capability than anything
else a tower can do.

The distinction the deeper detector draws, one-off versus continuous, is the
whole point. A single location request during an emergency call is ordinary. A
standing request that reports where you are every few seconds, for as long as it
lasts, is what continuous tracking actually looks like, and it is raised to
Medium for exactly that reason.

## When it fires harmlessly

- **Emergency calls.** The protocol's intended use. A call to an emergency
  number can legitimately involve a location request and report.
- **Carrier location services.** Some carriers use network-based location for
  lawful services, which can produce these messages in ordinary operation.
- **Any lawful location-based feature** your device or carrier runs may exercise
  the same protocol.

A hotspot sitting on a desk should see this rarely, so an unexplained warning is
worth attention, but "worth attention" is not "proof." And there is a caveat on
this detector heavier than the false-positive list: **it has never been
confirmed against a recording of a real network making one of these requests.**
Its correctness was established against reference encodings of the protocol (see
Precise behavior), not against live traffic, because the fork's test devices do
not produce LPP location requests to capture. Weigh a warning as a genuine but
unproven-in-the-wild signal, and read [How We Validate
Detectors](./validation.md) and [Reading Warnings Without
Panicking](../concepts/interpreting-warnings.md) alongside it.

## How it works

The positioning protocol (LPP) travels inside ordinary core-network transport
messages. When one arrives, the basic detector reads the fixed front of the LPP
message by hand, enough to tell which kind of message it is (a location
request, a location report, a routine capability exchange, assistance data) and
which conversation it belongs to. It deliberately reads no further, and if the
front cannot be read cleanly it says so rather than guessing, because a false
"your location was requested" is worse than an honest "an LPP message we could
not read."

The deeper detector reads on, into the body of a location request or report, to
recover two things: *which* positioning method the network asked for, a
precise satellite fix, tower-timing, or a coarse cell estimate, and, above all,
whether the request is for a single report or for **periodic** reporting on a
timer. Every field it reads sits at a fixed position with no variable-length
content in front of it, which is what makes reading it by hand safe; the moment
a field would sit behind something whose length the code cannot compute, it
stops.

Both detectors group messages by their conversation (LPP calls it a
transaction), so a periodic session that reports for an hour raises **one**
warning rather than thousands of identical ones, the first report warns, and
the repeats become informational notes until the conversation ends.

### Why the warnings are Low, not Informational

A design point worth stating, because it explains the severities. Rayhunter
never writes a report row whose events are all informational
([Severity](../severity.md) covers why). A location detector that marked
everything informational would therefore be *invisible*, it could never appear
in a report on its own. So the two messages that actually move location
information, a request, and a report, warn at Low, which is the lowest level
that still gets written and seen. Everything genuinely routine around them
(capabilities, assistance data) stays informational. The deeper detector then
lifts the continuous-tracking case from Low to Medium, because that one is worth
more than a single locate.

## Precise behavior

- **Code identifiers:** `lpp_location_request` (basic) and
  `lpp_location_tracking` (deep).
- **Source:** `lib/src/analysis/lpp.rs`; both analyzer version 1.
- **Severity:**
  - Basic: Low for a location request (downlink) or a location report (uplink),
    once per transaction. Informational for repeats, capability exchanges,
    assistance data, aborts, errors, and any LPP message whose prefix cannot be
    read. Generic transport messages that are not LPP at all produce nothing:
    the analyzer stays silent on them rather than emitting an informational note
    that could ride along on another detector's row.
  - Deep: Medium for a request asking for periodic (continuous) reporting; Low
    for a one-off request or a position report; Informational for repeats, for a
    device declining a request, and for abnormal-direction messages.
- **Deduplication:** per LPP transaction, keyed by initiator and transaction
  number. The entry is dropped when the transaction ends (or aborts), so a genuinely
  new transaction that reuses the number warns afresh. Bounded by the key space.
- **What it deliberately ignores:** the basic detector reads only the fixed
  message prefix; the deep detector reads request/response bodies only at fixed
  offsets and stops at the first field it cannot size. Neither decodes the full
  message, and undecodable input produces an informational note, never a
  location warning.
- **Independence:** the two run independently. The deep one warns even if the
  basic one is switched off, and the deep one can be turned off alone on a
  device very short on memory while keeping the basic awareness.
- **Validation, stated plainly.** The byte layouts were derived by hand from
  the protocol and then verified against encodings produced by pycrate's
  reference implementation of 3GPP TS 36.355. That check **caught a real
  one-bit error** during development (a missed extension bit that shifted every
  following field) and a mis-sized optional-field bitmap, which is exactly why
  vectors from an independent encoder are the ground truth. The detector is also
  exercised end to end by the "network set up continuous location tracking
  (LPP)" demonstration scenario. It has **not** been confirmed against a real
  network's LPP session.

## Configuration

Both enabled by default. Keys `lpp_location_request` and `lpp_location_tracking`
under `[analyzers]`, or the "Location asked for by the network" and "Continuous
location tracking" toggles on the settings page. Leaving both on is fine; on a
device very short on memory, `lpp_location_tracking` can be turned off alone to
save the extra decoding work while keeping the basic awareness.
[Configuration](../configuration.md) covers applying analyzer toggles.

## Sources

- **The exposure.** Shaik et al., NDSS 2016, for LTE location leaks, and EFF's
  white paper on measurement reports, [Sources and Further
  Reading](../references.md).
- **The protocol.** 3GPP TS 36.355 (LTE Positioning Protocol): the message
  structure, positioning methods, and periodic reporting this detector reads.
  3GPP TS 24.301 (NAS): the Generic NAS Transport that carries LPP.
- **In this book.** [Making Your Phone Report Its
  Position](../concepts/attack-location.md) for the attack, [How We Validate
  Detectors](./validation.md) for what the reference-encoder validation is
  worth, and [Location Requested on 2G (RRLP)](./rrlp.md) for the 2G
  counterpart.
