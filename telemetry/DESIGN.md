# Community dataset: design

How a Rayhunter unit can contribute what it saw to a community-run collection,
what leaves the device in each case, and how the collection side is built so
that a breach of it exposes as little as possible.

This answers [EFForg/rayhunter#108](https://github.com/EFForg/rayhunter/issues/108)
("Telemetry") and the map idea in
[discussion #673](https://github.com/EFForg/rayhunter/discussions/673). The
maintainers said three things there that shape everything below:

1. EFF will not host a collection server, and is not interested in doing so
   (cooperq, #673). So this is a design for a **community-run** service, with
   the code in this repository so that anybody can run one.
2. "I never want EFF to be the one that gives up a bunch of user data in a data
   breach" (cooperq, #108). So the service is built to hold **as little
   plaintext as possible**, and the raw captures it holds are encrypted to a
   key the server never has.
3. Warnings on their own are misleading; researchers need the raw recording
   for context (wgreenberg, #108 and #227). So there are **two tiers**, the
   second carrying the raw capture, and nothing is published without a person
   looking at it first.

A contributor to fadeproject.org added, on
[#154](https://github.com/EFForg/rayhunter/issues/154), that tower identities
are the one thing worth keeping (they let a tower be compared with where it is
supposed to be), that subscriber identities must go, and that tower identity
*plus signal strength* can place a person and so should reach analysts but not
the public. The tiers follow that advice.

## In one paragraph

A unit that has opted in builds a bundle for each finished recording that
raised a warning, encrypts it on the device to the collection service's public
keys, signs it with a key of its own, and uploads it later, over the owner's
Wi-Fi by default, never from the place the warning happened. The **summary**
tier is the existing shareable download: identifiers zeroed, raw capture left
out, home network and Wi-Fi details removed, plus a location rounded to a
grid the owner chooses. The **full** tier adds the raw capture, encrypted to a
second key that lives offline; the internet-facing server stores it and cannot
read it. A person reviews every submission before it appears on the public
site, which is a static page: a master list of interesting captures, a map,
and the summary files for download.

## What leaves the device

Nothing, unless the owner turns this on. When they do:

| | Summary tier | Full tier (after acknowledging the risk) |
|---|---|---|
| Analysis report (`.ndjson`): which detector fired, when, on which packet | yes | yes |
| Capture as PCAP with the device's own IMSI, IMEI and temporary identity zeroed | yes | yes |
| Raw capture (`.qmdl`), identifiers intact | **no** | yes, encrypted to the offline key |
| Redaction report (how many identifiers were removed, how many messages examined) | yes | yes |
| Device details: model, chipset, firmware build, Rayhunter version, clock readings, free storage | yes | yes |
| Home network (the SIM's carrier) | **no** | yes |
| Wi-Fi client state and network name | **no** | **no** (never; the name is the owner's home) |
| Cells heard: MCC, MNC, tracking area, cell identity, PCI, EARFCN, first and last seen | yes | yes |
| Signal strength per cell | **no** | yes |
| Location | one point, rounded to the owner's chosen grid (default about 10 km; also 1 km, exact, or none) | the recorded track, at the same chosen precision |
| Recording name and notes typed by the owner | **no** | only if a second box is ticked |
| Recordings that raised no warning | **no** (optional) | **no** (optional) |
| Recordings containing demo warnings | **never** | **never** |
| The recording currently being written | **never** | **never** |

Location is the one thing that identifies a *person* rather than a *device*,
which is why it is a separate choice with a coarse default rather than a
consequence of the tier. A cell identity already says roughly where the
device was, at the resolution of a tower's coverage; a 10 km grid point adds
nothing an attacker could not get from the cell identity, and it makes a map
possible.

The **summary tier is the shareable zip that already exists** in this fork,
with location added. That matters: the redaction it applies has been argued
over already (see `doc/sharing-findings.md`), and it is honest about what it
does not find. The bundle carries a redaction report rather than a claim of
cleanliness.

## What the device does

```
recording closes ──► analysed ──► eligible? ──► (wait: age, Wi-Fi) ──► build bundle
                                     │                                     │
              not eligible when:     │                     summary.zip (+ capture.zip)
              - current recording    │                                     │
              - contains demo data   │                            encrypt each part on device
              - excluded by owner    │                            (HPKE, to the server's keys)
              - already sent         │                                     │
              - no warning           │                            sign the manifest
                (unless opted in)    │                                     │
                                     ▼                                     ▼
                              status page               POST manifest ─► PUT parts ─► finalize
```

**Eligibility.** A recording is a candidate once it has closed, been analysed,
is older than `min_age_secs` (default one hour), raised at least
`min_severity` (default Low) or `include_clean_recordings` is on, is not
excluded, contains no demo warning, and has not already been submitted.

**Timing and route.** The default is `network = "wifi_only"`: the unit uploads
only while its Wi-Fi client is joined to a network, optionally only to named
ones (`allowed_networks`). That is the "when it gets back to the home network"
case. `network = "any"` also uploads over the cellular modem when the unit
has service. The one-hour minimum age plus Wi-Fi-only means the default never
uploads from the place where the warning happened, and never through the
tower that raised it. Both are deliberate: a device that phones home through
an IMSI catcher announces itself to the operator of the catcher.

**Bundle.** Built into a spool directory beside the recordings, from the same
code path as the shareable zip. The summary tier reuses `generate_redacted_pcap_data`
and `RecordingSidecar::redacted()`; nothing about redaction is reimplemented.
A `telemetry.json` inside the bundle carries what the server indexes:
detector events, cells heard, the coarse location, the redaction counts, and
a list of the bundle's contents.

**Encryption.** Each part is encrypted on the device with HPKE (RFC 9180,
DHKEM(P-256), HKDF-SHA256, ChaCha20-Poly1305) in a streamed chunk format, so
a 100 MB capture never has to sit in memory on a 160 MB device. The summary
part is encrypted to the service's **ingest** key, which the internet-facing
server holds so it can index submissions as they arrive. The capture part is
encrypted to the service's **archive** key, which the server does not hold.
See `telemetry/format` for the exact layout.

**Signing.** Every unit generates a P-256 key of its own, stored under the
auth directory (mode 0700) beside its TLS key. Submissions are signed with
it. The key is rotated every 30 days by default, so the server can tell
"the same unit sent these three things this month" (which it needs for rate
limiting and for honouring a withdrawal) but cannot follow one unit for a
year. Old keys are kept on the device so that a submission can still be
withdrawn after its key rotated.

**Withdrawal.** Every submission can be withdrawn from the device: a signed
request that makes the server delete the payload and drop the entry from the
next publication. The manifest and the withdrawal are signed by the same key.

**Server keys.** The unit fetches the service's keys from
`/.well-known/rayhunter-telemetry` once, when the owner enters the URL and
presses *Check server*, shows their fingerprints, and pins them in
`config.toml`. If the service later presents different keys, uploads stop and
the settings page says why. Trust on first use, with the change visible.

**Consent record.** The manifest carries the tier and, for the full tier, the
time the owner acknowledged what it contains. The server refuses full-tier
submissions without it. A submission can therefore always be shown to have
been sent with the owner's explicit choice.

## What the service does

```
   unit ──TLS──► reverse proxy ──► rayhunter-collector serve ──► data/submissions/<id>/
                                       │                            manifest.json (+ signature)
                              verifies signature,                   parts/summary.enc  ─┐ decrypted on arrival
                              checks sizes and tiers,               parts/capture.enc  ─┼─ stays encrypted
                              rate limits,                          summary.zip, summary.json
                              decrypts the summary part             state.json (pending/received/verified/…)

   maintainer ──► rayhunter-collector review <id> --status verified --tag interesting
              ──► rayhunter-collector decrypt <id> --archive-key (offline machine)
              ──► rayhunter-collector publish --out site/   ──► static HTML + JSON + GeoJSON
```

**One binary, three roles**, so a community operator has one thing to
deploy:

- `serve`: the ingest API. Plain HTTP behind a TLS reverse proxy (Caddy,
  nginx). It holds the ingest private key and nothing else secret.
- `review`, `list`, `show`, `decrypt`: the triage tools. `decrypt` needs the
  archive private key and is meant to run on a machine that is not the
  server.
- `publish`: writes a static site. Nothing dynamic faces the public.

**What a breach of the server yields.** Summary bundles awaiting review (the
same content their owners chose to publish), opaque ciphertext for every raw
capture, the ingest private key (which decrypts *future* summary uploads, not
past captures), and submitter public keys. No IMSIs, no raw captures, no
home networks, no IP addresses (the server does not store them beyond the
rate limiter's memory). This is the property that answers the maintainers'
objection.

**Moderation before publication.** A submission is `pending` until its parts
arrive, `received` once verified and decrypted, and only `verified` after a
person marks it so. `publish` writes verified submissions only. Reviewers
attach tags (`interesting`, `vulnerable`, `confirmed`, `false-positive`,
`needs-capture`) and a note. This is how "warnings alone are misleading"
is handled: the master list is curated, and each entry carries the
reviewer's judgement alongside the device's.

**Abuse.** Signatures stop forged or altered submissions. Per-key and per-IP
limits, hard size caps per part, and a disk quota stop floods. A key can be
banned. Because keys are pseudonymous and rotate, a ban is a month-long
inconvenience to an abuser, which is enough given that nothing is published
without review anyway.

**The public site.** A master list (sortable, filterable by severity, tag,
country and operator from the MCC and MNC, device, date), a map of coarse
points with a popup per submission, a page per submission with its events,
cells and downloads, and machine-readable feeds (`submissions.json`,
`map.geojson`, `submissions.csv`). Everything on it came from a summary
bundle whose owner chose to send it, after a reviewer looked at it.

## Rejected alternatives, and why

- **Uploading nearby Wi-Fi networks for location.** The set of access points
  around a unit pinpoints a home. The browser's own geolocation, which this
  fork can use because it serves HTTPS ([#1047](https://github.com/EFForg/rayhunter/issues/1047)),
  gives the same "Wi-Fi location" without any BSSID leaving the device, and
  fixed coordinates cover stationary units. So BSSIDs are never sent.
- **A central account per user.** Accounts link everything a person sends.
  Rotating device keys give the server what it needs and no more.
- **Auto-publishing, or a live map of raw warnings.** Would fill the site with
  aircraft-landing false positives and demo data, and hand any observer a
  live feed of where detectors are. Review first, and coarse location.
- **Encrypting the summary tier to the offline key too.** Then nothing could
  be indexed without a human decrypting each one, and the service would not
  scale past a few submissions a week. The summary is what the owner chose
  to make public; protecting it from the server it is being published by
  buys little.
- **Pinning the server's TLS key.** The payload is already encrypted and
  signed end to end, so TLS is transport, not the security boundary. A pin
  would only add a way for a key rotation to strand every unit.
- **Sending only a webhook on warning, no data** (RootLUG on #108). Useful
  for personal alerting, and `ntfy_url` already does it. It is not a dataset.

## Open questions for the maintainers

- Whether EFF would link to a community-run instance from the docs, and what
  review standard it would want to see before it did.
- Whether the summary tier should also drop RRC measurement reports from the
  PCAP. They carry signal strengths per cell. The #154 thread argued they
  are the most valuable thing for analysis; the summary tier keeps them and
  relies on coarse location instead.
- A name. "RayHive" was suggested on #108.

## Where the code is

| Piece | Path |
|---|---|
| Envelope format: keys, streamed HPKE, manifest signing | `telemetry/format` (crate `rayhunter-telemetry-format`) |
| Device side: config, eligibility, bundle, upload worker, API | `daemon/src/telemetry/`, `daemon/src/config.rs` (`[telemetry]`) |
| Settings page | `daemon/web/src/lib/components/ConfigForm.svelte` (Community tab) |
| Collection service and publisher | `telemetry/collector` (binary `rayhunter-collector`) |
| User documentation | `doc/community-dataset.md`, `doc/concepts/community-dataset.md` |
| Operator documentation | `telemetry/collector/README.md` |
