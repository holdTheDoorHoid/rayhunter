# Encryption Disabled (RRC)

Watches for a tower asking your phone to communicate with no encryption at all.

## What you would see

A High warning that the tower proposed a null cipher, that it asked your phone
to talk to it with the encryption turned off. It appears among a recording's
warnings and turns the device status line to the warning colour. Nothing shows
on your phone itself: there is no open-padlock icon for an unencrypted cellular
connection the way a browser warns about an insecure page.

## Why it matters

Everything your phone sends over the air travels through open space where a
listener with the right radio can pick it up; encryption is what turns that
into noise. A tower proposing *no* encryption is asking for the noise to be
plain speech again, for the conversation between your phone and it to be
readable by whoever operates it. Real networks essentially never do this
outside a test lab. A fake tower does it as a matter of course, because it does
not hold the cryptographic keys real encryption would require, and proposing
"no encryption" is the way around that. [Turning Off
Encryption](../concepts/attack-encryption.md) is the full explanation; this
detector watches for it at the radio layer, the link between your phone and the
tower.

## When it fires harmlessly

The honest causes are few but real, and worth knowing because this is a
high-severity warning:

- **Emergency calls.** A phone with no SIM, or a SIM the network cannot
  authenticate, must still be able to reach an emergency number. There is no
  shared secret to build encryption from in that case, so the standard permits
  the call to proceed unencrypted. An emergency call can therefore legitimately
  involve a null cipher.
- **Test and lab environments.** Equipment set up for testing often runs
  without encryption on purpose. If you or anyone nearby is running such a
  setup, treat a warning here as expected until you have ruled that out.
- **Genuine misconfiguration.** A badly configured network, or one operating
  where strong encryption is restricted, could in principle produce this. Both
  are uncommon.

Outside those cases, a real network has little reason to switch encryption off,
which is why this is one of the more meaningful warnings Rayhunter raises. Even
so, this repository records no measured false-positive rate for it, and a
warning is a strong lead to preserve and corroborate rather than proof on its
own. [Reading Warnings Without
Panicking](../concepts/interpreting-warnings.md) is the method.

## How it works

Before your phone and a tower exchange anything meaningful, they agree how to
protect it: the tower sends a message naming an encryption method, and your
phone switches it on. Among the methods both sides recognise is one that means
*no encryption*. This detector reads the messages where that choice is made and
checks whether the named method is the null one.

It looks in more than one place, because encryption can be set or changed at
several points in a connection: the initial security-setup message, and the
reconfiguration messages that can reset it during a handover or when a second
radio link is added. Wherever the null method appears in those, the detector
warns.

## Precise behavior

- **Code identifier:** `null_cipher`.
- **Source:** `lib/src/analysis/null_cipher.rs`; analyzer version 1.
- **Severity:** High.
- **What it inspects:** the RRC *Security Mode Command*, and the *Connection
  Reconfiguration* message across its handover configuration (both intra-LTE
  and inter-radio-technology handover), its secondary-cell-group configuration,
  and its later 5G-related handover variants. The null ciphering algorithm
  (EEA0) in any of these triggers the warning.
- **Deduplication:** none; each qualifying message is evaluated on its own.
- **What it deliberately ignores:** integrity-protection settings (it checks the
  ciphering algorithm, not the integrity algorithm), and the deeper
  core-network encryption, which is the separate [Encryption Disabled
  (NAS)](./nas-null-cipher.md) detector's job.
- **Validation:** inherited from upstream and exercised by the "tower switched
  encryption off (RRC null cipher)" demonstration scenario. No real-capture
  validation is recorded in this repository.

## Configuration

Enabled by default. The key is `null_cipher` under `[analyzers]`, or the
"Encryption switched off by the tower" toggle on the settings page.
[Configuration](../configuration.md) covers applying analyzer toggles.

## Sources

- **The mechanism.** EFF's white paper *Gotta Catch 'Em All*, on how catchers
  deal with encryption, [Sources and Further Reading](../references.md).
- **The protocol.** 3GPP TS 33.401 (SAE security architecture): which ciphering
  and integrity algorithms are permitted, including the null algorithm EEA0 and
  the narrow circumstances it is meant for. 3GPP TS 36.331 (E-UTRA RRC): the
  Security Mode Command and Connection Reconfiguration messages this detector
  reads.
- **In this book.** [Turning Off
  Encryption](../concepts/attack-encryption.md), and its companion detector
  [Encryption Disabled (NAS)](./nas-null-cipher.md).
