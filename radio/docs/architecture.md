# Wireless surveillance detection: daemon, IPC and data architecture

Proposal, informed by the Phase 0 measurements in
[`capability-report-rc400l.md`](./capability-report-rc400l.md) and by the
maintainers' review history on EFForg/rayhunter#888 and #1042.

## The constraint that decides the shape

On PR #888 cooperq asked for exactly this separation:

> I think that wifi should be a separate crate outside of rayhunter … I think
> that rayhunter should not be this tightly coupled with the wifi handling.

Phase 0 adds a second, harder reason. Creating a scan interface needs
`CAP_NET_ADMIN`, and on the RC400L only init-started processes have it. So the
radio side needs its own init-managed process regardless of how we feel about
coupling — which conveniently gives us the isolation the requirements ask for.

## Processes

```
                 ┌───────────────────────────────┐
   /dev/diag ───►│ rayhunter-daemon              │
                 │  cellular parse + heuristics  │
                 │  recording store, web UI      │◄─── browser
                 └───────────────┬───────────────┘
                                 │ reads (optional, best-effort)
                                 │  • unix socket for live status
                                 │  • NDJSON sidecar for evidence
                 ┌───────────────▼───────────────┐
                 │ rayhunter-radio-daemon        │
                 │  owns rhscan0, paces scans    │
                 │  matches signatures           │
                 │  persistence scoring          │
                 │  writes evidence sidecar      │
                 └───────────────┬───────────────┘
                                 │
              ┌──────────────────┼──────────────────┐
              │                  │                  │
      host Wi-Fi scan     host BLE (absent)   companion radio
                                                (future ESP32)
```

`rayhunter-daemon` never links the radio hardware paths. If
`rayhunter-radio-daemon` crashes, hangs, exhausts memory or cannot bring up an
interface, the cellular daemon does not notice: it reads the socket if it is
there and shows the wireless panel as unavailable if it is not. Cellular
detection is unaffected by construction, not by careful coding.

The radio daemon is capability-gated. If no supported radio is found it exits
cleanly and the UI says so, rather than showing an empty panel that looks like
"nothing is out there".

## Crates

| Crate | Contents | Depends on hardware |
|---|---|---|
| `rayhunter-radio` | observation model, signatures, matching, evidence format, persistence scoring | no |
| `rayhunter-radio-daemon` (next) | interface lifecycle, scan pacing, IPC server, log rotation | yes |

`rayhunter-radio` is deliberately I/O-free and hardware-free, which is what
makes it testable on a laptop — the 69 tests in it run with no device
attached. It is also what lets a companion radio be added later without the
analysis engine learning anything new.

Today its modules are `mac`, `observation`, `signature`, `scan`, `evidence`.
If the crate grows enough to warrant splitting, the seams are already along
the `radio-observation` / `radio-signatures` / `radio-analysis` lines the
requirements suggest.

## The observation boundary

Everything the analysis engine consumes is a `RadioObservation`:

```rust
pub struct RadioObservation {
    pub source: ObservationSource,   // HostWifiScan | HostWifiMonitor
                                     // | HostBle | External(String)
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub observation_count: u32,
    pub payload: ObservationPayload, // Wifi | Ble | RemoteId
}
```

The engine never asks where an observation came from in order to decide what
to do with it. `source` is recorded for provenance and for explaining coverage
gaps to the user, not for branching detection logic. That is what keeps an
Orbic hardware limitation from becoming an architectural one: an ESP32 feeding
`External("esp32-s3")` BLE observations reaches exactly the same signature
matcher, persistence scorer and evidence writer.

Address fields are individually optional because capture methods differ. A BSS
scan fills in `bssid` and `transmitter`; a monitor-mode capture fills in the
frame's addr1/2/3. Signatures name the field they apply to, so a rule written
for the transmitter never silently matches an access point that was merely
answering the target — the `addr1`/`addr3` false-positive mode flock-you
documents.

## IPC

A Unix-domain socket at `/data/rayhunter/radio.sock`, mode `0660`, owned
root:root. Local-only by construction: no TCP, no external listener, nothing
to misconfigure onto the WAN.

Line-delimited JSON, request/response, with three verbs to start:

- `status` — radio present, capture method in use, capability gaps, scan rate,
  counters
- `current` — active alerts and nearby known devices
- `persistent` — devices above the persistence threshold, with scores and
  reasons

The daemon is the server; `rayhunter-daemon` is a client and treats every call
as failable. A missing socket, a timeout or a malformed reply degrades the
wireless panel and nothing else.

## Evidence on disk

Per-recording NDJSON sidecar, `<recording-id>-radio.ndjson`, beside the
existing `<recording-id>.ndjson`.

Not QMDL: wireless observations are not diag messages and forcing them in
would corrupt the meaning of that format. Not the analysis NDJSON either,
because that file is rewritten when a recording is re-analysed, and a
re-analysis of cellular data cannot regenerate radio observations — they would
be silently destroyed. This is the concern cooperq raised on #1042:

> One way to do this would be to write it to the ndjson file but I think those
> currently get erased when we do a rescan. Another way would be to write it to
> its own separate wifi log which might be a better idea.

The sidecar is included in recording export ZIPs and served over WebDAV
alongside the other per-recording files.

Each record carries the signature `id` and the `rule_version` in force, so an
alert stays explicable after the signature pack changes.

## Retention

Following-detection has to watch devices that match nothing, which is in
tension with not building a permanent record of every person nearby. The
resolution:

- A device that matched a signature, or crossed a persistence threshold, is
  written with its real address. It is the subject of the report.
- Every other device is written as `anon-<hash>`, a per-session salted
  pseudonym. Stable within the session so persistence can be scored, useless
  outside it. The salt is generated per run and never stored.
- Detailed recording of all addresses is available but off by default.

Verified on hardware: a live scan of 15 nearby networks produced 15 evidence
records and zero retained MAC addresses.

The pseudonym is FNV-1a, and the code says plainly that this is an obfuscation
boundary rather than a security one — a 48-bit address space is trivially
enumerable given the salt. Its job is keeping bystanders out of a durable log.

## Confidence, severity and the display

Confidence and severity are separate axes on purpose.

*Confidence* is how sure the detector is: `INFO` for a single weak indicator
such as a vendor prefix, up to `HIGH` for several independent signals forming
a specific device fingerprint. *Severity* is how loudly to say it. A
high-confidence identification of a benign device should not raise a severe
alert.

Two rules the code enforces rather than merely documents, checked by tests
over the shipped pack:

- A signature whose only condition is a MAC prefix cannot claim more than
  `INFO` confidence or `Informational` severity.
- A signature cannot claim `HIGH` confidence from a single condition.

Every detection carries `matched_fields`, a human-readable reason per
satisfied condition, surfaced verbatim in the UI. A user is never asked to
trust an unexplained verdict:

```
Possible Flock device — MEDIUM confidence
  • transmitter 70:c9:4e:… matches prefix 70:c9:4e
  • wildcard (zero-length) SSID observed
  • RSSI -61 dBm
```

The device display threshold is configurable and defaults above
informational, so an OUI-only match never turns the Orbic's screen red.

### Escalation

`DetectionLog` deduplicates per device *and per signature*, but a strictly
higher confidence always reports:

```
observe(dev, weak)   -> New
observe(dev, weak)   -> Suppressed
observe(dev, strong) -> Escalated { from: Info }
```

This preserves the property flock-you is careful about: an early weak hit must
never stop a later fingerprint-confirmed one from being surfaced.

## Signatures as data

A versioned JSON pack, schema version rejected rather than
best-effort-parsed. Builtin and user packs stay separate files, matching the
direction the maintainers proposed on #1042. No executable content, no regex
in v1 — exact, prefix, substring and bounded glob only, so there is no
ReDoS surface to reason about.

Prefixes are measured in **nibbles**, not bytes, because
`70:b3:d5:7c:b` — the KeyW allocation cooperq raised — is nine hex digits. The
parent block `70:b3:d5` is the IEEE Registration Authority's shared MA-S range,
so matching three bytes there would fire on unrelated hardware. This is the
detail that most shapes the matching code.

Every signature records its provenance, and anything not independently
verified ships `enabled: false`.

## Web UI

A distinct **Wireless** category, never mixed into the cellular heuristics.
Technology badges `CELL` / `WIFI` / `BLE` / `REMOTE ID`. Drone Remote ID gets
its own section, because a compliant drone nearby is not evidence of
surveillance and must not be presented as though it were.

Per #888's review, the whole panel is feature-gated and hidden on devices
without radio support rather than shown broken.

## Sequencing

Small, reviewable, independently testable steps, none of them a giant PR:

1. `rayhunter-radio` crate: model, signatures, evidence, scan parsing. *(done —
   69 tests, no hardware needed)*
2. Builtin signature pack with provenance and verification tests. *(done)*
3. `rayhunter-radio-daemon`: interface lifecycle, scan pacing, bounded caches,
   log rotation, IPC server.
4. `rayhunter-daemon` client + web UI panel, feature-gated.
5. Evidence sidecar wired into export/WebDAV, with a test that re-analysis
   does not destroy it.
6. Persistence scoring with environment-change context, location off by
   default.
7. Companion-radio ingest over the same observation API.

Steps 1 and 2 are complete and green. Step 3 is the next piece of real work,
and it is the one that has to earn its resource budget on a 160 MB device.
