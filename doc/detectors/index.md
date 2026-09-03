# Detector Reference

Rayhunter's detectors are the individual checks that watch your phone's traffic
for the patterns described in [How These Attacks Actually
Work](../concepts/attacks.md). This page lists every one of them in a single
table, and then explains the column that matters most: how thoroughly each has
actually been tested.

Each detector has its own page with the full picture, what it watches for,
when it fires harmlessly, and how it works. Start here to see the whole set and
decide which pages to read; follow a link for the detail.

## The honest column: validation status

Before the table, the thing to understand about it. A detector that looks for
the right pattern is only as trustworthy as the testing behind it, and the
detectors here have been tested to genuinely different depths. This book marks
three levels, and uses them plainly:

- **Reference-encoder vectors.** The detector was checked against messages
  produced by an independent, authoritative implementation of the protocol, strong evidence that it reads the bytes correctly, and in one case this
  caught a real one-bit error during development. It is **not** evidence that
  the detector behaves well on a live network, which produces messier and more
  varied traffic than any encoder.
- **Demo and synthetic only.** The detector is exercised by Rayhunter's own
  demonstration scenarios or hand-built test messages, which confirm the
  pipeline works end to end but are made by the same project that wrote the
  detector.
- **Real traffic, status not established here.** Several detectors were
  inherited from the upstream project and have real-world history there, but
  this repository does not record a specific real-capture validation, so this
  book does not claim one. Where that is the case, the detector's page says so
  rather than implying more.

The single most important honesty in this whole book lives in this column:
**none of the fork's location detectors has been confirmed against a recording
of a real network** making the request it looks for. They are marked
accordingly, here and on their pages, and [How We Validate
Detectors](./validation.md) explains why that gap exists and what would close
it.

## Every detector

| Detector | Code identifier | Severity it can raise | Default | Validation |
|---|---|---|---|---|
| [Identity Requested Without Authentication](./imsi-requested.md) | `imsi_requested` | High, Low, Informational | On | Inherited; demo-exercised. **Validated by EFF against a real cell-site simulator** (fires on a genuine attack); field false-positive rate still unknown. |
| [Redirected to 2G](./connection-redirect-downgrade.md) | `connection_redirect_2g_downgrade` | High, Informational | On | Inherited; demo-exercised. Real-capture status not established here. |
| [2G/3G Advertised Above 4G](./priority-2g-downgrade.md) | `lte_sib6_and_7_downgrade` | High, Informational | On | Inherited; demo-exercised. Earlier versions were noisy; current is stricter. |
| [Encryption Disabled (RRC)](./null-cipher.md) | `null_cipher` | High | On | Inherited; demo-exercised. |
| [Encryption Disabled (NAS)](./nas-null-cipher.md) | `nas_null_cipher` | High | On | Inherited; demo-exercised. |
| [Incomplete System Information](./incomplete-sib.md) | `incomplete_sib` | Informational only | On | Inherited. Informational-only, so invisible in a report on its own. |
| [Location Requested (LPP)](./lpp.md) | `lpp_location_request` | Low, Informational | On | **Fork.** Reference-encoder vectors. Never confirmed against real traffic. |
| [Continuous Location Tracking (LPP)](./lpp.md) | `lpp_location_tracking` | Medium, Low, Informational | On | **Fork.** Reference-encoder vectors. Never confirmed against real traffic. |
| [Location Requested on 2G (RRLP)](./rrlp.md) | `rrlp_location_request` | Low, Informational | On | **Fork.** Reference-encoder vectors. Never seen against real 2G traffic. |
| [A Tower That Seems to Have Moved](./timing-advance.md) | `timing_advance` | Low | On | **Fork.** Unit-tested; silent on the Orbic and any modem that reports no timing advance. Never seen against a real cell-site simulator. |
| [Identity Taken, Then Forged Authentication (FlashCatch)](./flash-catch.md) | `flash_catch` | High, Medium, Informational | On | **Fork.** Synthetic messages and a demo scenario, built from the paper's description of the exchange. Never confirmed against a recording of the attack. |
| [Identity Exposure Diary](./imsi-requested.md) | `diagnostic_analyzer` | Informational only | On | Inherited. Informational-only, so invisible in a report on its own. |
| Alert on Every Tower (testing) | `test_analyzer` | Low | Off | A self-test, not a detector. Fires on every tower beacon; leave it off while hunting. |

Notes on reading the table:

- **Severity** is what each detector *can* raise; most findings sit at the
  lower end. [Severity, and What It Means](../severity.md) defines the levels
  and explains why the two "Informational only" detectors never appear in a
  report by themselves.
- **Default** is whether the detector is on in a fresh configuration. Every
  real detector ships on; only the tower self-test ships off. A configuration
  written by an older version comes up with any newer detectors on, so an
  update never silently leaves one disabled.
- **The two LPP rows** are two separate detectors reading the same messages to
  different depths, and share one page. The deeper one can be switched off
  alone on a device very short on memory.

## The fork's additions

Four of the detectors above, the two LPP location detectors, the RRLP one, and
the timing-advance check, are additions in this fork of Rayhunter and are not
present upstream. They are also the ones carrying the "never confirmed against
real traffic" caveat, which is not a coincidence: they are new, and newly
written code watching for something the project's own test devices cannot
readily produce (a live location request, or, for timing advance, a real
distance jump the Orbic cannot even measure). [What It
Adds](../fork/features.md) lists the fork's changes as a whole, and each
detector's page states its fork status and validation directly.

## Where to next

- [How Detection Works](../heuristics.md) is the shared background on how the
  detectors read messages at all.
- [How We Validate Detectors](./validation.md) explains the standard of
  evidence behind the validation column.
- [Reading Warnings Without Panicking](../concepts/interpreting-warnings.md) is
  how to weigh what any of these detectors tells you.
