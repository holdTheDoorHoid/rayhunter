# Encryption Disabled (NAS)

Watches for the operator's core network turning encryption off after your phone
has already been verified.

## What you would see

A High warning that the core network requested a null cipher, that the
operator's network, not the local tower, asked for encryption to be switched
off. It appears among a recording's warnings and turns the device status line
to the warning colour. As with the radio-layer check, nothing shows on your
phone.

## Why it matters

This is the more serious of Rayhunter's two encryption warnings, and the reason
is worth understanding. The [radio-layer null cipher](./null-cipher.md) is the
ordinary fake-tower move: a nearby impostor with no keys proposes no encryption
because that is the only path it has. This warning is different. It fires on
encryption being switched off **at the core-network layer, after your phone has
already proven its identity**, and reaching that point means whoever is doing
it holds genuine cryptographic key material for your SIM, which an ordinary
fake tower cannot obtain.

That points somewhere more troubling than a briefcase in a van. It can indicate
cooperation from the carrier itself, or an attack on the signalling network
that carriers use to exchange subscriber information between each other. Either
way, this is not the signature of a cheap catcher; it is the signature of
something with real access. [Turning Off
Encryption](../concepts/attack-encryption.md) develops the two-layer picture.

## When it fires harmlessly

- **Genuine carrier misconfiguration.** A network that is set up wrong could
  request this. It is uncommon, but possible.
- **Jurisdictions where encryption is restricted.** Some places limit the
  encryption networks may use by law, which could produce a null-cipher request
  as normal local behaviour. If you are somewhere that does this, weigh the
  warning with that in mind.

Both causes are rare, which is part of why this warning is weighted as
seriously as it is, but "rare" is not "never," and this repository records no
measured false-positive rate for it. A warning here is a strong and unusual
signal to preserve and, if your situation warrants, to get expert eyes on;
[Reading Warnings Without Panicking](../concepts/interpreting-warnings.md) and
[Legal and Personal Risk](../concepts/risk.md) are the pages for deciding what
to do next.

## How it works

A mobile connection is encrypted at two levels: the radio link between your
phone and the tower, and the deeper conversation between your phone and the
carrier's core network, which passes through the tower. Each level has its own
setup step, and each recognises a "no encryption" option.

This detector reads the core-network setup step, the point where the core
tells your phone which encryption to use for their conversation, and checks
whether the method named is the null one. It exists separately from the
radio-layer check because these are two different negotiations at two different
layers, and null encryption at the deeper one carries the graver implication
above. The two detectors together tell you not only *that* encryption was
disabled but *where*, which is most of what makes the finding readable.

## Precise behavior

- **Code identifier:** `nas_null_cipher`.
- **Source:** `lib/src/analysis/nas_null_cipher.rs`; analyzer version 1.
- **Severity:** High. The greater seriousness relative to the radio-layer check
  is in what it implies, real key material or a signalling-network attack, not in a higher number; both are High.
- **What it inspects:** the NAS *Security Mode Command* (the core network's
  encryption-setup message), for the null ciphering algorithm EEA0.
- **Deduplication:** none; each qualifying message is evaluated on its own.
- **What it deliberately ignores:** the radio-layer encryption, which is the
  separate [Encryption Disabled (RRC)](./null-cipher.md) detector, and
  integrity settings.
- **Validation:** inherited from upstream and exercised by the "tower switched
  encryption off (NAS null cipher)" demonstration scenario. No real-capture
  validation is recorded in this repository.

## Configuration

Enabled by default. The key is `nas_null_cipher` under `[analyzers]`, or the
"Encryption switched off by the core network" toggle on the settings page.
[Configuration](../configuration.md) covers applying analyzer toggles.

## Sources

- **The mechanism.** EFF's white paper *Gotta Catch 'Em All*, on how catchers
  deal with encryption, [Sources and Further Reading](../references.md).
- **The protocol.** 3GPP TS 33.401 (SAE security architecture): the permitted
  algorithms including the null cipher EEA0. 3GPP TS 24.301 (NAS for EPS): the
  Security Mode Command this detector reads.
- **In this book.** [Turning Off
  Encryption](../concepts/attack-encryption.md), and its companion detector
  [Encryption Disabled (RRC)](./null-cipher.md).
