# How the Community Dataset Protects You

Rayhunter can contribute recordings to a community-run collection. This page is
about why it is built the way it is: what a contribution reveals, what it is
designed not to reveal, and where the trust actually sits. For the steps, see
[Contributing Recordings to a Community Dataset](../community-dataset.md).

## What you would see

Your device records a warning on a Tuesday. On Wednesday evening, once it is
back on your home WiFi, it sends a bundle to a service you chose. Some days
later, after a person has looked at it, an entry appears on that service's
public site: the date, the kind of warning, the device model, the network
identities of the towers it heard, and a point on a map about ten kilometres
wide. Anyone can download the capture from that entry and open it in Wireshark.
Your name is nowhere. Your phone number is nowhere. Your IMSI is a row of zeros.

## Why it matters

A single warning proves little. The same warning from thirty devices in one
city over one weekend is a pattern, and patterns are what researchers, lawyers
and reporters can act on. That is why people keep asking for this, from the
first month the project existed.

The same bundle, sent carelessly, is a record of where you were and which SIM
you carry. That is why the project's maintainers have refused to host a
collection themselves, and why this design starts from what must never leave
rather than from what would be nice to have.

## How it works

**Two versions, and you choose.** The shareable version is the same bundle as
the *zip (shareable)* download: identifiers zeroed, raw capture left out. The
full version adds the raw capture, for researchers who need every message. The
full version is off by default, needs your explicit acknowledgement, and is
encrypted to a key that is not on the internet-facing server at all.

**Location is a separate choice.** A tower's identity already says roughly where
you were, at the resolution of that tower's coverage. Rounding your location to
about ten kilometres adds nothing an attacker could not get from the tower, and
it makes a map possible. You can send a finer point, an exact one, or none.

**Encrypted before it leaves.** Each bundle is encrypted on your device to the
service's public keys, and signed with a key your device made for itself. The
server can open the shareable version, because it has to index it. It cannot
open the full version: that key lives offline with whoever reviews captures.
Someone who breaks into the server gets shareable bundles awaiting review, which
their owners meant to publish anyway, and ciphertext.

**Never from the scene.** Uploads wait an hour after a recording closes and, by
default, only happen over WiFi. A device that phoned home the moment it saw a
suspicious tower would do so *through* that tower, announcing to whoever runs it
that a detector is present and reporting. The default makes that impossible.

**A person looks first.** Nothing appears on the public site until a reviewer
has marked it. Warnings on their own are misleading; some fire on aircraft
landing, some on ordinary network behaviour. The reviewer's tags and note
appear beside the device's own report.

**You can take it back.** Every contribution can be withdrawn from the
recording's row in the history. The device keeps the key that signed each one,
so a withdrawal can be signed even after the key has been replaced.

**A short-lived identity.** The device's signing key is replaced every 30 days.
The service can group one device's contributions for a month, which it needs
to refuse floods, and cannot follow the device for a year.

## The precise details

- The format, the keys and the encryption are in `telemetry/format` in the
  repository. Encryption is HPKE (RFC 9180) with P-256, HKDF-SHA256 and
  ChaCha20-Poly1305, in chunks so a large capture never sits in memory.
  Signatures are ECDSA P-256 over the exact bytes sent.
- What the device decides and builds is in `daemon/src/telemetry/`. The
  shareable bundle is produced by the same functions as the *zip (shareable)*
  download: `generate_redacted_pcap_data` and `RecordingSidecar::redacted`.
- The service is `rayhunter-collector` in `telemetry/collector`, with its own
  README for operators. The public site it writes is static HTML and JSON.
- The design document, with the alternatives considered and rejected, is
  `telemetry/DESIGN.md`. The upstream discussions are
  [EFForg/rayhunter#108](https://github.com/EFForg/rayhunter/issues/108),
  [#154](https://github.com/EFForg/rayhunter/issues/154) and
  [discussion #673](https://github.com/EFForg/rayhunter/discussions/673).

## What this does not protect against

- **The person running the service.** They see everything in the shareable
  version, unreviewed, and if they hold the archive key, everything in the
  full version. Choose a service run by someone you would hand a capture to
  in person.
- **Your own choices.** An exact location with a timestamp says where you were
  and when. The interface warns; it does not stop you.
- **The redaction's limits.** Identifiers are found where the device announces
  itself in plain messages. A ciphered message hides them from Rayhunter as
  much as from anyone else. See [Sharing What You Find](../sharing-findings.md).
- **Traffic analysis.** Someone watching your home connection sees that your
  device talks to a collection service, and roughly how much. They do not see
  what.
