# Location Requested on 2G (RRLP)

The same idea as the location checks for modern networks, but for the older 2G
network that phones fall back to.

This detector is an **addition in this fork** and is not present upstream. It is
also the first thing in Rayhunter to read 2G signalling at all.

## What you would see

A Low warning that a 2G network asked your device for its location, or that your
device reported its position over 2G. It appears among a recording's warnings;
routine 2G positioning chatter (assistance data, acknowledgements) is recorded
as informational notes rather than warnings. On a device that never uses 2G,
this detector stays silent.

## Why it matters

Being [pushed onto 2G](../concepts/attack-downgrade.md) is itself a known
surveillance move, because 2G's protections are weak. Once a phone is there,
this older location protocol is how it can be pinpointed, the 2G ancestor of
the modern positioning protocol covered in [Location Requested (LPP)](./lpp.md).
Watching for it means a switch to 2G followed by a location request does not go
unnoticed. The two location detectors are companions: the same concern applies
wherever your phone still uses 2G, and a downgrade-then-locate sequence across
the two is a more coherent story than either event alone.

[Making Your Phone Report Its Position](../concepts/attack-location.md) is the
full explanation of the location attack in both its modern and 2G forms.

## When it fires harmlessly

- **Emergency calls on 2G.** As with the modern protocol, a location request is
  legitimate when locating an emergency call, including on 2G.
- **Lawful 2G location services**, where a carrier still runs them.

Two honesty notes matter more than the list, and they pull in opposite
directions:

- **It has never been seen against real 2G traffic.** The fork's test devices
  are LTE-only and do not produce 2G RRLP messages to capture, so this detector, though carefully built and tested against reference encodings, is the least
  exercised-in-the-wild of them all. Treat a warning as a real but entirely
  unproven-in-practice signal.
- **A false positive cannot come from a wrong guess about the message type
  alone.** Detection requires *both* a valid 2G transport header *and* a
  decodable location message inside it. A message that merely resembles the
  outer framing, without a real positioning message within, is rejected. That
  design makes an accidental false alarm from misidentification unlikely, even
  though the real-world false-positive rate is unmeasured.

[How We Validate Detectors](./validation.md) explains what the reference-encoder
testing is and is not worth, and [Reading Warnings Without
Panicking](../concepts/interpreting-warnings.md) is the method for weighing one.

## How it works

On 2G, a location message does not travel on its own. The network wraps it
inside an ordinary 2G signalling message (a Radio Resource "Application
Information" message), which is what reaches Rayhunter as raw 2G bytes. This
detector reads that outer wrapper to confirm it really carries a location
message and to find where that message begins, then reads the front of the
location message itself to tell a location *request* from a *response* or from
routine assistance data.

Both layers are short, fixed headers read by hand. The detector reads only
enough to say that a positioning exchange happened and which way it went; it
does not decode the body of the location message. If either layer fails to
decode, nothing is reported, which is the property that makes a
misidentification-only false positive impossible.

## Precise behavior

- **Code identifier:** `rrlp_location_request`.
- **Source:** `lib/src/analysis/rrlp.rs`; analyzer version 1.
- **Severity:** Low for a location request (`measurePositionReq`) or a position
  report (`measurePositionRsp`). Informational for assistance data, assistance
  acknowledgements, protocol errors, and unrecognised components.
- **Deduplication:** **none.** Unlike the LPP detectors, which warn once per
  transaction, this one evaluates every qualifying message on its own, so a long
  2G positioning session could produce one Low warning per message. This is a
  known difference from the LPP behavior and is worth bearing in mind when
  reading a 2G capture. <!-- NEEDS INPUT: is the absence of per-session
  deduplication intentional (2G sessions expected rare/short), or worth aligning
  with the LPP per-transaction rule before this is offered upstream? -->
- **What it deliberately ignores:** anything that is not a location-bearing
  Application Information message (including the similar message type that
  carries emergency-warning data rather than positioning, which is explicitly
  rejected), and the body of the location message beyond its leading component
  type.
- **The reusable half.** Making 2G messages reach an analyzer at all is a fork
  change underneath this detector: the internal representation of a 2G message
  now carries its raw bytes, where before it was an empty placeholder. Any
  future 2G detector builds on that.
- **Validation, stated plainly.** The 2G transport framing was verified against
  pycrate_mobile's implementation of 3GPP TS 44.018, and the location-message
  front against pycrate's 3GPP TS 44.031, both as round-tripped test vectors. A
  truncation sweep confirms malformed input never crashes the detector, and a
  demonstration scenario ("2G network asked the device for its location")
  exercises the whole path from raw bytes to warning. It has **not** been
  confirmed against a real 2G capture.

## Configuration

Enabled by default. The key is `rrlp_location_request` under `[analyzers]`, or
the "Location asked for on 2G (older networks)" toggle on the settings page. On
a device that never uses 2G it costs nothing, staying silent.
[Configuration](../configuration.md) covers applying analyzer toggles.

## Sources

- **The exposure.** Shaik et al., NDSS 2016, and EFF's white paper on
  measurement reports, [Sources and Further Reading](../references.md).
- **The protocol.** 3GPP TS 44.031 (Radio Resource LCS Protocol, RRLP): the
  location message this detector reads. 3GPP TS 44.018 (GSM/EDGE RRC): the
  Application Information message that carries RRLP on 2G.
- **In this book.** [Making Your Phone Report Its
  Position](../concepts/attack-location.md) for the attack, [Location Requested
  (LPP)](./lpp.md) for the modern counterpart, and [How We Validate
  Detectors](./validation.md) for the evidence standard.
