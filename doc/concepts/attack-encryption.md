# Turning Off Encryption

Everything your phone sends over the air travels through open space, where
anyone with the right radio can pick it up. What stops a listener from
understanding it is encryption: your phone and the network scramble their
conversation so that, to anyone in between, it is noise. Take the scrambling
away and the noise becomes plain speech again, your messages, who you are
calling, readable by whoever is listening.

That is what this technique does. It does not break the encryption, which
would be hard. It convinces your phone not to use any. This page explains how
a conversation about encryption can end in no encryption at all, and why a
fake tower needs that outcome. It maps to two of Rayhunter's detectors, one
for each layer where it can happen:
[Encryption Disabled (RRC)](../detectors/null-cipher.md) and
[Encryption Disabled (NAS)](../detectors/nas-null-cipher.md).

## What you would see

Nothing on your phone. There is no open-padlock icon for a call the way a web
browser warns you about an insecure page. The negotiation happens in the
control messages your phone and the tower exchange before your traffic starts
flowing, invisibly, and the result is not surfaced to you. This is one of the
starker gaps between how the web taught people to think about encryption and
how phones actually behave: on a phone, you are not told.

## Why it matters

With encryption switched off, the protection described above is gone. Whoever
operates the tower your phone is talking to can read what passes between you
and it, and in some cases change it as it goes by
([Sources and Further Reading](../references.md), EFF's white paper on how
catchers deal with encryption). Not metadata about your traffic, the traffic.

The two detectors that catch this are not redundant, and the difference
between them is a difference in how serious the finding is:

- **The tower turning off radio encryption** is bad, and it is the ordinary
  signature of a fake tower.
- **The core network turning off encryption, after your phone has already
  proven its identity,** is worse, and points somewhere different. Reaching
  that point means whoever is doing it holds genuine cryptographic key
  material for your SIM, something an ordinary fake tower cannot obtain. It
  can indicate cooperation from the carrier itself, or an attack on the
  signalling network that carriers use to exchange subscriber information
  between each other. The [NAS detector page](../detectors/nas-null-cipher.md)
  develops this, and it is the more alarming of the two for a reason worth
  understanding.

## How it works

### The conversation that sets the lock

Before your phone and the network exchange anything meaningful, they agree on
how to protect it. The network sends a message that says, in effect, "let us
secure this conversation using the following method," and names an encryption
algorithm from a list both sides support. Your phone switches on that method
and replies in kind. From [How Cell Networks Work](./cell-networks.md), this
is the *Security Mode Command* step, and normally it is where the lock clicks
shut.

### The "no lock" option, and why it exists

Among the algorithms both sides recognise is one that means *no encryption at
all*. It is a real, standardised option, not an oversight, and it is worth
knowing why the specification includes it, because the reason is legitimate
and the abuse borrows that legitimacy.

The clearest honest use is an emergency call. A phone with no SIM, or a SIM
the network cannot authenticate, must still be able to call an emergency
number. There is no shared secret to build encryption from in that case, so
the standard permits the call to proceed unencrypted rather than fail. The
"no encryption" algorithm also has uses in testing. The 3GPP security
architecture (TS 33.401, [Sources and Further Reading](../references.md))
defines these null algorithms and the narrow circumstances they are meant for.

### Why a fake tower reaches for it

A real network, having proven itself with your SIM's secret, uses that same
secret to drive genuine encryption. A fake tower cannot, because it does not
hold the secret, this is the same wall it hit when it skipped authentication
in [Capturing Your Identity](./attack-identity.md). Real encryption is exactly
the thing it has no key for.

So it takes the one path around the wall the standard leaves open: propose the
"no encryption" algorithm. If your phone accepts, the conversation proceeds in
the clear, and the operator can read it without ever having held a key. Turning
encryption off is not a sophisticated break. It is the simplest way for
someone with no key to arrange a readable conversation, which is why real
networks essentially never propose it outside a lab and fake ones do it as a
matter of course.

### Two layers, two detectors

Encryption on a mobile network is set up at two levels, and the "no
encryption" option exists at both, which is why Rayhunter has two detectors
here rather than one.

- The **radio layer (RRC)** protects the link between your phone and the
  tower. Null encryption proposed here is the ordinary fake-tower move, caught
  by [Encryption Disabled (RRC)](../detectors/null-cipher.md).
- The **core-network layer (NAS)** protects the conversation between your
  phone and the carrier's core, which passes through the tower. Null
  encryption proposed here, after authentication has succeeded, is the more
  serious case above, caught by
  [Encryption Disabled (NAS)](../detectors/nas-null-cipher.md).

Seeing either is significant. Seeing the NAS one is a stronger and stranger
signal than the RRC one, and the pages for each explain how to weigh them.

## The precise details

- **The negotiation** is the LTE *Security Mode Command*; the "no encryption"
  algorithm is **EEA0**, the null ciphering algorithm. Which algorithms are
  permitted, including the null ones and the conditions attached to them, is
  set by 3GPP TS 33.401 ([Sources and Further Reading](../references.md)).
- **The RRC detector** (`lib/src/analysis/null_cipher.rs`) inspects the
  *Security Mode Command* and *Connection Reconfiguration* messages for EEA0,
  including in handover and dual-connectivity configurations, and warns at
  high severity. **The NAS detector** (`lib/src/analysis/nas_null_cipher.rs`)
  inspects the NAS *Security Mode Command* for the null cipher and also warns
  at high severity, the greater seriousness is in what it implies, described
  above and on its page, not in the severity number.
- **The rare honest cause.** A genuinely misconfigured carrier, or one
  operating where strong encryption is restricted by law, could also produce
  a null-cipher finding. Both are uncommon, and the detector pages say so
  where it matters. Weighing that possibility is the work of
  [Reading Warnings Without Panicking](./interpreting-warnings.md).
- **What falls outside.** Rayhunter sees the negotiation aimed at this device.
  It cannot report on encryption between other phones and a tower, nor on an
  interception that reads traffic without changing the encryption settings at
  all, [What This Tool Cannot Tell You](./limitations.md).

## Where to next

[Making Your Phone Report Its Position](./attack-location.md) is the fourth
technique, and the most precise: rather than infer where you are, it asks your
phone to say.
