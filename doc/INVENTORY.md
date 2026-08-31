# Repository inventory for documentation

Phase 0 of `DOCS_PROMPT.md`. Built 2026-08-31 on branch `device-display-colors`,
merge-base with upstream `87872e3` (same day; upstream `main` has since moved to
`681f708` — see §5 and §7).

This file is a working document for doc-writing sessions, not a book page. It is
not listed in `SUMMARY.md` and should not be.

---

## 1. Detectors

Everything in `lib/src/analysis/`. "Invisible alone" refers to the rule in
`AnalysisRow::is_empty` (`lib/src/analysis/analyzer.rs`): a row whose events are all
Informational is never written, so a detector that only emits Informational events
appears in a report only when another detector warns on the very same packet.

Report format is version 3 (`REPORT_VERSION` in `analyzer.rs`), bumped by the packet
explorer's `packet_num` field on rows.

### imsi_requested — `lib/src/analysis/imsi_requested.rs`
- **Code id / config key:** `imsi_requested` · **get_name:** "Identity (IMSI or IMEI) requested in suspicious manner" · **UI title (`heuristics.ts`):** "Identity requested without proof of identity" · **Version:** 4 · **Origin:** upstream
- **Examines:** NAS EMM messages (attach request, extended service request, identity request, attach complete, authentication response, service reject, both detach requests, TAU reject, attach reject with cause analysis), RRC UL-CCCH connection/reestablishment requests, and SIB1 (to learn the tower's PLMN list for roaming judgement).
- **Severities:** High — identity requested after auth accept; identity requested with no attach request; disconnect after identity request without auth accept *on the home network* (PLMN match). Low — the same disconnect pattern when PLMNs suggest roaming; attach reject with cause "EPS services and non-EPS services not allowed" (likely inactive SIM). Informational — identity request with no follow-up within 50 packets (`TIMEOUT_THRESHHOLD`), invisible alone.
- **Dedup:** state machine; one event per suspicious transition, timeout counter resets.
- **Tests:** none in the file. Validation against real traffic unrecorded — NEEDS INPUT (upstream heritage suggests real-world exposure, but nothing in this repo proves it).
- **Known false positives:** aircraft on approach (per `heuristics.ts`), roaming, inactive SIMs.
- **Note:** upstream commits newer than our merge-base (`73505f9`..`681f708`) teach this analyzer to read the home PLMN from the SIM. Not in this fork yet; will change the roaming false-positive logic on next rebase. See §7.

### connection_redirect_2g_downgrade — `lib/src/analysis/connection_redirect_downgrade.rs`
- **Code id:** `connection_redirect_2g_downgrade` · **get_name:** "Connection Release/Redirected Carrier 2G Downgrade" · **UI title:** "Pushed down to a 2G network" · **Version:** 1 · **Origin:** upstream
- **Examines:** RRC DL-DCCH `RRCConnectionRelease` carrying `redirectedCarrierInfo`.
- **Severities:** High when the redirect target is GERAN (2G); Informational for any other target (message includes the carrier info debug string).
- **Dedup:** none; fires per matching message.
- **Tests:** none in the file. Exercised by demo scenario "pushed down onto a 2G network". Real-traffic validation unrecorded.
- **Source:** Lin Huang, "Forcing a targeted LTE cellphone into an eavesdropping network" (HITBSecConf), per code comment.

### lte_sib6_and_7_downgrade — `lib/src/analysis/priority_2g_downgrade.rs`
- **Code id:** `lte_sib6_and_7_downgrade` · **get_name:** "LTE SIB 6/7 Downgrade" · **UI title:** "Old networks advertised as better than nearby 4G" · **Version:** 2 · **Origin:** upstream
- **Examines:** SIB3/SIB5 (LTE reselection priorities), SIB6 (UTRA FDD/TDD priorities), SIB7 (GERAN priorities), evaluated each time a SIB1 marks a new broadcast cycle.
- **Severities:** High when the highest legacy (2G/3G) priority beats the highest LTE priority; Informational when legacy priorities were advertised but no LTE priority was seen at all.
- **Dedup:** per SIB1 cycle; priority state resets on every SIB1.
- **Tests:** none in the file. Demo scenario "2G advertised as a better choice than nearby 4G". `heuristics.ts` records that earlier versions raised many false alarms and v2 is stricter — the detector page should tell that history.
- **Source:** heuristic T7, Shinjo Park, "Why We Cannot Win" (per code comment).

### null_cipher — `lib/src/analysis/null_cipher.rs`
- **Code id:** `null_cipher` · **get_name:** "Null Cipher" · **UI title:** "Encryption switched off by the tower" · **Version:** 1 · **Origin:** upstream
- **Examines:** RRC DL-DCCH `SecurityModeCommand`, and `RRCConnectionReconfiguration` in three places: handover security config (intra-LTE and inter-RAT), SCG configuration (r12), and the v1530 5G-related handover variants.
- **Severities:** High on EEA0 anywhere above.
- **Dedup:** none; per message.
- **Tests:** none in the file. Demo scenario "tower switched encryption off (RRC null cipher)".

### nas_null_cipher — `lib/src/analysis/nas_null_cipher.rs`
- **Code id:** `nas_null_cipher` · **get_name:** "NAS Null Cipher Requested" · **UI title:** "Encryption switched off by the core network" · **Version:** 1 · **Origin:** upstream
- **Examines:** NAS EMM Security Mode Command; fires on cipher algorithm EEA0.
- **Severities:** High.
- **Dedup:** none; per message.
- **Tests:** none in the file. Demo scenario "tower switched encryption off (NAS null cipher)".

### incomplete_sib — `lib/src/analysis/incomplete_sib.rs`
- **Code id:** `incomplete_sib` · **get_name:** "Incomplete SIB" · **UI title:** "Tower broadcasting only a fragment of its details" · **Version:** 2 · **Origin:** upstream
- **Examines:** SIB1 whose `schedulingInfoList` has fewer than 2 entries.
- **Severities:** **Informational only** — invisible alone. Enabled by default, yet on its own it can never appear in a live recording's report.
- **Dedup:** none; per matching SIB1.
- **Tests:** none in the file.
- **Discrepancy:** `heuristics.ts` presents it like a warning-producing detector and gives it no `informational` tag (the diagnostic analyzer has one). Docs and UI need to agree on what this detector can actually show. See §7.

### lpp_location_request — `lib/src/analysis/lpp.rs` (fork addition)
- **Code id:** `lpp_location_request` · **get_name:** "LPP Location Request" · **UI title:** "Location asked for by the network" · **Version:** 1
- **Examines:** NAS DL/UL Generic NAS Transport with container type 1 (LPP, 3GPP TS 36.355). Decodes only the fixed UPER prefix by hand: presence bits, transaction ID, message kind. Deliberately reads nothing further; undecodable prefixes report "LPP message seen, but its type could not be read" (Informational) rather than guessing.
- **Severities:** Low, once per (initiator, transaction number) — network location request downlink; device location report uplink; also a location request travelling uplink (abnormal flow). Informational — repeats within a transaction, capability exchanges, assistance data, abort, error, non-LPP container (type 2), unreadable prefix.
- **Dedup:** `warned_transactions` map keyed (initiator, transaction id), entry dropped on endTransaction or Abort so a new transaction reusing the number warns afresh. Bounded at 512 entries by the key space.
- **Tests:** extensive, in-file. Byte vectors produced by pycrate's reference TS 36.355 encoder and round-tripped through its decoder; the vectors caught a real one-bit layout error (missed extension bit on LPP-TransactionID) during development. Demo round-trip exercises the full diag→gsmtap→IE path ("network set up continuous location tracking (LPP)" scenario). **Never confirmed against a real network's LPP session.**
- **Deliberately ignores:** message bodies, sequence numbers beyond skipping, acknowledgements.

### lpp_location_tracking — `lib/src/analysis/lpp.rs` (fork addition)
- **Code id:** `lpp_location_tracking` · **get_name:** "LPP Location Tracking" · **UI title:** "Continuous location tracking" · **Version:** 1
- **Examines:** same LPP container; decodes request/response bodies at fixed bit offsets: positioning methods requested (A-GNSS satellite / OTDOA tower timing / E-CID cell / external EPDU), whether reporting is **periodic**, and whether the response carried an estimate or declined.
- **Severities:** Medium — periodic (continuous) tracking request, the surveillance signature. Low — one-off location request; device position report; uplink request (abnormal). Informational — repeats within a transaction, device declining the request, downlink report (abnormal).
- **Dedup:** own `warned_transactions` map, same rules; runs independently of the basic analyzer so either can be disabled alone (split exists so memory-constrained devices can keep the cheap one).
- **Tests:** same reference-encoder vectors, including the offset checks for the method bits and the 7-optional bitmap of `CommonIEsRequestLocationInformation` (mis-sized as 5 during development; the vectors caught it). Never confirmed against real traffic.

### rrlp_location_request — `lib/src/analysis/rrlp.rs` (fork addition)
- **Code id:** `rrlp_location_request` · **get_name:** "RRLP Location Request" · **UI title:** "Location asked for on 2G (older networks)" · **Version:** 1
- **Examines:** 2G (GSM) Layer 3 signalling — the first analyzer to consume `InformationElement::GSM`, which this fork made carry real bytes. Requires a valid GSM RR APPLICATION INFORMATION header (PD 0x6, message type 0x38, APDU ID 0 = RRLP per TS 44.018 §9.1.53) *and* a decodable RRLP APDU front (TS 44.031), so a wrong message-type guess cannot alone produce a false positive.
- **Severities:** Low — measure position request; measure position response. Informational — assistance data, assistance ack, protocol error, unrecognised component.
- **Dedup:** **none** — every matching message fires. (The APDU reference number is decoded but not used to group; a long 2G positioning session would produce one Low event per message. Contrast with LPP's per-transaction rule. Worth stating on the detector page, and see §7.)
- **Tests:** in-file vectors from pycrate_mobile TS 44.018 (transport framing) and pycrate TS 44.031 (APDU), ETWS non-RRLP rejection, truncation-never-panics sweep. Demo round-trip ("2G network asked the device for its location (RRLP)"). **Never seen against real 2G traffic — the project's test devices are LTE-only.**

### diagnostic_analyzer — `lib/src/analysis/diagnostic.rs`
- **Code id:** `diagnostic_analyzer` · **get_name:** "Diagnostic detector for messages which might lead to IMSI exposure" · **UI title:** "Connection diary" · **Version:** 1 · **Origin:** upstream
- **Examines:** NAS EMM messages that can expose identity: all identity requests; TAU/attach/service rejects with specific cause values; MT detach requests that are not IMSI-detach. Based on the "Marlin" paper (per description).
- **Severities:** **Informational only** — invisible alone; its entries surface when another analyzer warns on the same packet (which is common, since identity requests are exactly what `imsi_requested` warns about).
- **Discrepancy:** the `heuristics.ts` entry describes it as "records when your phone joins and leaves each tower", which is not what the code does. One of the two is wrong (the UI text, in my reading). See §7.

### test_analyzer — `lib/src/analysis/test_analyzer.rs`
- **Code id:** `test_analyzer` · **get_name:** "Test Analyzer" · **Version:** 1 · **Origin:** upstream · **Default: off**
- Fires Low on every SIB1 (i.e. on every tower beacon), reporting the cell identity. Exists to prove the pipeline works; drowns real warnings if left on.

### Not an analyzer but part of the analysis story
- `analyzer.rs` — `Harness` (wiring, packet numbering, per-event " (packet N)" suffix), `AnalyzerConfig` (defaults: everything on except `test_analyzer`), `is_deliberately_unanalysed` (2G/3G/LTE-MAC traffic is not reported as parse failures; fork addition), report (de)serialization across versions 1–3.
- `information_element.rs` — GSMTAP → IE conversion. LTE RRC subtypes and plain NAS parse into trees; `GsmtapType::Um` (2G) becomes `InformationElement::GSM` carrying raw bytes (fork change; previously an empty stub). UMTS and 5G variants exist but nothing produces or consumes them.
- Demo (`daemon/src/demo.rs`, fork): 9 scenarios injected through the real diag→gsmtap→IE→analyzer path, gated by `demo_mode`, every event prefixed as a demo (prefixing happens in `daemon/src/analysis.rs` at write time). Scenario names: NAS null cipher, RRC null cipher, connection release redirect, SIB downgrade, IMSI-after-auth, identity-no-attach, LPP continuous tracking, RRLP location, IMEI demanded.

---

## 2. Configuration

Read from `config.toml` (path given as the daemon's only argument), parsed in
`daemon/src/config.rs`. Every field is `#[serde(default)]`, so a missing key silently
takes its default — that is the upgrade path and also the failure mode: a **misspelled
key is ignored without any error**. A top-level key appended after the `[analyzers]`
table lands inside that table and is silently dropped (`dist/config.toml.in` ends with
`[analyzers]`; the installer's `set_top_level_bool` exists for this reason).

`POST /api/config` replaces the whole file and restarts the daemon (~60s).
`ServerState.config` is a boot-time snapshot; `web_users` is the one runtime-mutable
piece and lives in its own lock.

Defaults below are from `Config::default()` in code. (\*) marks keys **absent from
`dist/config.toml.in`**, so a user reading the shipped template never learns they
exist — the configuration reference must cover them.

| Key | Type | Default | Fork? | What it does / what breaks |
|---|---|---|---|---|
| `qmdl_store_path` | string | `/data/rayhunter/qmdl` | | Recording storage. Wrong path on a read-only mount: no recordings. |
| `port` | u16 | 8080 | | Web UI port; daemon panics at boot if it cannot bind. |
| `debug_mode` | bool | false | | No diag thread, no display, no recording; store must already exist. |
| `device` (\*) | enum | `orbic` | | Selects display driver + battery/wifi paths. Wrong value: blank or broken display, wrong sysfs paths. Written by the installer. |
| `ui_level` | int 0–5, 128 | 1 | 5 is fork | 0 invisible, 1 subtle line, 2 orca demo, 3 EFF logo, 4 high-visibility fill, 5 custom GIF (fork), 128 trans flag. |
| `colorblind_mode` | bool | false | | Green→blue substitution on the status line. |
| `display_colors.{paused,recording,warning_low,warning_medium,warning_high}` (\*) | `#rrggbb` strings | unset | fork | Per-state color overrides; malformed hex falls back to built-in color rather than breaking the display. One-bit displays ignore. |
| `status_bar_height` (\*) | u32 or unset | unset (2px) | fork | Status line height, clamped to screen height at draw time; ignored by high-visibility. |
| `display_gifs.{state}` (\*) | filenames | unset | fork | Original filename per state; actual image lives in `gif_store_path` as `<state>.gif` (may hold a PNG; sniffed by magic bytes). Uploading a file without this key set does nothing visible. |
| `gif_store_path` (\*) | string | `/data/rayhunter/gifs` | fork | Where uploaded images live. |
| `demo_mode` (\*) | bool | false | fork | Enables `POST /api/demo-warning` and the web UI demo button. Writes clearly-labelled fake warnings into a real recording. |
| `key_input_mode` | int 0/1 | 0 | | 1 = double-tap power starts a new recording. |
| `web_users` (\*) | array | empty | fork | HTTP Basic accounts. Empty = open interface (historic behaviour, update-safe). Managed via `/api/web-users`, not by editing the file. |
| `terminal_enabled` (\*) | bool | false | fork | Web terminal. Only settable at install time (`--enable-terminal`); deliberately not settable from the web UI. Must sit above the first table header. |
| `show_subscriber_identity` (\*) | bool | false | fork | Whether the web API discloses this device's own IMSI/IMEI/temporary identity. Off by default on purpose: the interface may be unauthenticated. |
| `keep_screen_on` (\*) | int 0/1/2 | 0 | fork | 0 never, 1 always, 2 only while plugged in. Orbic-implemented; other devices ignore it. Plugged-in test reads `/sys/class/power_supply/usb/online`. |
| `pause_display_on_keypress` (\*) | bool | true | fork | Button press shrinks the overlay to the thin line for 20s so the device's own screens (wifi password!) can be read. |
| `ntfy_url` | string or unset | unset | | Push notifications via ntfy. |
| `enabled_notifications` | array | `["Warning","LowBattery"]` | | Which notification types fire. |
| `auto_check_updates` | bool | **true in code, `false` in the template** | | GitHub release check + UI notice. The template/code default disagreement is worth resolving — see §7. |
| `clock_sync_mode` | int 0/1/2 | 2 (prompt) | | Clock drift handling; offset is memory-only and lost on restart. |
| `[analyzers]` table | 11 bools | all true except `test_analyzer` | 3 fork keys | See §1. Missing keys default **on** (upgrade path, tested in `config.rs`). |
| `min_space_to_start_recording_mb` | u64 | 1 | | Below this, recording will not start. |
| `min_space_to_continue_recording_mb` | u64 | 1 | | Below this, recording stops. |
| `auto_delete_clean_recordings` (\*) | bool | false | fork | When space runs low, delete analysed, warning-free, un-named, already-uploaded-or-not-pending recordings, oldest first. Named recordings are protected. |
| `max_recording_size_mb` (\*) | u64 or unset | unset | fork | Rotate recording at size. |
| `max_recording_minutes` (\*) | u64 or unset | unset | fork | Rotate recording at age; with both set, first wins. |
| `gps_mode` (\*) | int 0/1/2 | 0 | | 0 off, 1 fixed coordinates, 2 API-fed. `/api/gps` 404s when off (the fork fixed the page hang this caused). |
| `gps_fixed_latitude` / `gps_fixed_longitude` (\*) | f64 | unset | | Used when `gps_mode = 1`; missing pair logs a warning and records nothing. |
| `wifi_enabled` | bool | false | | WiFi client mode. |
| `wifi_ssid` / `wifi_password` / `wifi_security` (\*) | strings | unset | | Managed via the web UI; credentials actually live in `wpa_sta.conf`, and `parse_config` overwrites these fields from it at boot (password never round-trips). |
| `dns_servers` | array or unset | unset (Quad9) | | Client-mode DNS. |
| `[webdav]` `url`,`username`,`password`,`upload_timeout_secs` (300),`poll_interval_secs` (3600),`min_age_secs` (86400),`delete_on_upload` (false) | table | url empty = off | | Background upload of finished recordings (.qmdl + .ndjson); entry marked uploaded or deleted locally. |

---

## 3. HTTP surface

All routes in `daemon/src/main.rs::get_router()`. **Auth:** one middleware
(`web_auth::require_auth`) wraps every route including static files: pass-through when
`web_users` is empty, otherwise HTTP Basic against the live account list. There is no
TLS; the interface itself says accounts are a second factor beyond the WiFi password,
not a secure channel. The OpenAPI spec (`cargo run --bin gen_api --features apidocs`)
is served via `doc/api-docs.md` + `swagger-ui.html`; new endpoints must be added to
`ApiDocs` in `daemon/src/lib.rs` or they silently vanish from it.

| Route | Method | Response / notes |
|---|---|---|
| `/api/pcap/{name}` | GET | PCAP-NG stream of a recording. |
| `/api/qmdl/{name}` | GET | Raw QMDL bytes. |
| `/api/zip/{name}` | GET | Zip of qmdl + pcap + report; filename uses the recording's display name (fork). |
| `/api/system-stats` | GET | JSON: load, uptime, temperature, disk, measured CPU (fork additions). |
| `/api/update-status` | GET | JSON release-check state. |
| `/api/qmdl-manifest` | GET | JSON manifest: entries carry `display_name`/`notes` (fork). |
| `/api/log` | GET | Daemon log text. |
| `/api/start-recording` · `/api/stop-recording` | POST | Control recording. |
| `/api/delete-recording/{name}` · `/api/delete-all-recordings` | POST | Deletion. |
| `/api/annotate-recording/{name}` | POST | Fork. Set display name (sanitized `\w{0,29}`) and notes. |
| `/api/analysis-report/{name}` | GET | NDJSON analysis report (versioned; see §1). |
| `/api/analysis` | GET | Analysis queue/status JSON. |
| `/api/analysis/{name}` | POST | Re-run analysis with current analyzers. |
| `/api/cell-info` | GET | Fork. Serving cell, neighbours, encryption, identity counters; subscriber identity values only when `show_subscriber_identity` (`server.rs:236`). Data is intermittent by nature — ML1 reports only arrive when the modem is active. |
| `/api/packets/{recording}` · `/api/packets/{recording}/{packet_num}` | GET | Fork. Packet explorer: windowed message list and single-packet detail decoded by the same path the detectors use. |
| `/api/demo-warning` | POST | Fork. 403-style refusal unless `demo_mode`; injects a labelled synthetic scenario. |
| `/api/config` | GET / POST | Full config JSON. POST **replaces everything and restarts the daemon (~60s)**; omitted fields reset to defaults. |
| `/api/web-users` · `/api/web-users/{username}/delete` | POST | Fork. Account management; hashes redacted on read; takes effect immediately. |
| `/api/display-gif/{state}` | GET / POST (size-capped) | Fork. Fetch/upload per-state image; `/delete` variant removes. |
| `/api/test-notification` | POST | Sends a test ntfy push. |
| `/api/wifi-status` | GET · `/api/wifi-scan` POST | WiFi client mode. |
| `/api/time` | GET · `/api/time-offset` POST | Clock drift check/sync (offset memory-only). |
| `/api/debug/display-state` | POST | Force a display state (not gated by `debug_mode`; used to verify display work). |
| `/api/debug/keypress` | POST | Fork. Simulate a button press (503 when no display). |
| `/api/terminal` | POST | Fork. Run one command as root; refused unless `terminal_enabled`. |
| `/api/gps` | GET / POST | GPS state; GET 404s when `gps_mode` is off (by design — the UI now tolerates it). |
| `/` → `/index.html`, `/{*path}` | GET | Embedded SvelteKit UI (built into the binary via `include_bytes!`). |

---

## 4. Device support

`Device` enum (`lib/src/lib.rs`): Orbic, Tplink, Tmobile, Wingtech, Pinephone, Uz801,
Moxee. Display dispatch in `daemon/src/main.rs`; installer modules under
`installer/src/`.

| Device | Display module | Installer | Device-specific notes |
|---|---|---|---|
| Orbic/Kajeet RC400L | `display/orbic.rs` (128×128 RGB565 framebuffer) | `orbic.rs`, `orbic_network.rs`, `orbic_auth.rs` | The reference device. Only implementation of `keep_screen_on` (sysfs `sleep_mode`/`bl_gpio`/`autosleep`). Framebuffer is shared with the stock UI, hence repaint-every-pass. Battery `is_plugged_in` reads `chg_en`, which is 0 on a full battery (known quirk). |
| Moxee Hotspot | shares `display/orbic.rs` | `moxee.rs` | Routed through the Orbic display path. |
| TP-Link M7350 (v3–v9) | `display/tplink.rs` → `tplink_framebuffer.rs` (color) or `tplink_onebit.rs` | `tplink.rs` | One-bit variants draw pixel-art status faces and ignore all color/GIF settings. Three doc pages (M7350/M7310/M7200) map to this one Device value. |
| TP-Link M7310 / M7200 | as above | as above | |
| T-Mobile TMOHS1 | `display/tmobile.rs` | `tmobile.rs` | |
| Wingtech CT2MHS01 | `display/wingtech.rs` | `wingtech.rs` | |
| UZ801 | `display/uz801.rs` | `uz801.rs` | Android-style paths for wpa_supplicant/hostapd. |
| PinePhone / PinePhone Pro | `display/headless.rs` (no display) | `pinephone.rs` | Headless. In this fork the profile doubles as the Quectel EG25-G modem port work — see §7 for how the docs should present it. |

Fork display features touch every module through one shared `update_ui` signature
(`button-pauses-the-overlay`); colors/heights/GIFs are implemented in
`generic_framebuffer.rs` and apply to framebuffer devices only.

---

## 5. Fork delta — `UPSTREAM.md` vs the actual diff

Diffed `87872e3` (merge-base)..HEAD: 103 files, ~16.5k insertions. Feature-by-feature,
`UPSTREAM.md`'s inventory checks out — every listed feature exists in the diff with the
files it names. Gaps found, in both directions:

1. **`subscriber-identity` is missing from `UPSTREAM.md` entirely.** `daemon/src/subscriber_id.rs` (313 lines, decodes EPS mobile identity IEs per TS 24.301 §9.9.3.12), identity-sent tracking in `cell_info.rs`, the `show_subscriber_identity` config gate, and the identity panel in the web UI. Commits `a6c2ddd`, `a72e703` (trailer `Feature: subscriber-identity`). Worse, `UPSTREAM.md`'s note under `neighbouring-cells (#326)` still claims "showing IMSI, TMSI and IMEI" is a part **we do not have** — now false. `UPSTREAM.md` needs an entry and that note needs correcting before Phase 4 uses it as the source of truth.
2. **`lib/src/diag/diaglog/ml1.rs`** (ML1 serving/neighbour measurement parsing) is part of `cell-site-panel` but not listed in its Files.
3. Several changed files are dark-mode variants covered by the blanket "nearly every component" note (`app.html`, `ActionErrors`, `AnalysisTable`, `ManifestTable`, `ClockDriftAlert`, `DownloadLink`, `ExpandableInput`, `LogView`, `Modal`) — fine, just noting the diff is accounted for.
4. **`daemon/src/analysis.rs`** changes are the demo-prefixing half of `demo-control` (not listed in its Files).
5. Claims in `UPSTREAM.md` I could not verify from the repo: the state of upstream issues/PRs (maintainer comments, who claimed what). Those live on GitHub and were taken on trust.
6. **Upstream has moved past our merge-base already** (7 commits, same day): reading the home PLMN from the SIM over AT commands for `imsi_requested` (`lib/src/plmn.rs`, `lib/src/sim/`), plus cleanups. Affects the imsi-requested detector page's false-positive story after the next rebase.

---

## 6. Undocumented behavior (user-visible, no obvious home yet)

The new `SUMMARY.md` gives a page to everything fork-added. What still has **no
dedicated page** and must be deliberately parked somewhere:

- **WebDAV upload, WiFi client mode, GPS, ntfy notifications, clock sync, update checks** (all upstream features): their home is `configuration.md` (kept from upstream) plus the new `configuration-reference.md`. No new pages needed, but the reference must not skip them.
- **`ui_level = 128` (trans flag)** and the demo orca: only discoverable in `config.toml.in` comments today. One line each in the configuration reference.
- **Key input / double-tap-power to start a recording**: config key exists; belongs in `device-display.md` or `using-rayhunter.md`.
- **Informational-only detectors are invisible alone** (`AnalysisRow::is_empty`): affects how users read reports and why "diagnostic" entries appear only next to warnings. Belongs in `severity.md` (its scaffold brief already says so).
- **The `check` CLI** (`rayhunter-check`, with `-j` JSON output): referenced by `analyzing-a-capture.md` (upstream page, kept) — verify it covers the JSON flag and the fork's analyzers.
- **Debug endpoints** (`/api/debug/*`): power-user/testing surface; a short note in `api-docs.md` or the porting page is enough.

---

## 7. Open questions

Blocking or shaping questions, per the Phase 0 instruction to be exhaustive. The
starred ones need an answer before Phase 1 starts; the rest before their phase.

1. ★ **Which device does `quick-start.md` commit to?** The tutorial picks exactly one. The Orbic RC400L is the obvious choice (project's own test hardware, upstream's reference device, cheapest and best-documented) — confirm.
2. ★ **Where will this book live, and under whose name?** `book.toml` still says `edit-url-template = github.com/efforg/rayhunter/...` (upstream), and the book title is upstream's. If the book documents the fork, the edit URL should point at the fork (`holdTheDoorHoid/rayhunter`) and the introduction needs a sentence about what this fork is. Publishing plan (GitHub Pages on the fork?) also decides whether relative links to repo files outside `doc/` are acceptable.
3. ★ **`diagnostic_analyzer` UI text is wrong** ("connection diary" vs. what the code does — flag identity-exposing messages). Fix `heuristics.ts` (my recommendation; the docs must describe the code either way), or is there history behind that wording?
4. **`incomplete_sib` presentation:** informational-only in code, presented as a warning-producer in the UI with no `informational` tag. Fix the UI entry, the code's severity, or just document the reality? (Docs will state the reality regardless.)
5. **RRLP has no dedup** while both LPP analyzers dedup per transaction. Intentional (2G sessions expected rare/short) or an oversight worth fixing before its detector page freezes the behavior in writing?
6. **PinePhone page:** present it as upstream does (PinePhone/Pro via headless profile), or document this fork's reality that the profile is also the EG25-G modem port under active work? Affects `pinephone.md` and `porting.md`.
7. **`auto_check_updates` default mismatch:** `false` in the shipped template, `true` in code (so any config missing the key gets update checks). Which is intended? The configuration reference has to print one answer.
8. **Rebase timing vs. imsi-requested page:** upstream's home-PLMN-from-SIM work (landed today) changes that detector's false-positive logic. Write the page against current fork code and revise after rebase (my recommendation), or rebase first?
9. **`imsi_requested` real-world validation:** is there a known real capture (upstream's or ours) this heuristic has been exercised against? The detector page must state its validation status and I could not determine it from the repo.
10. **Do we have any real recording containing LPP traffic?** The user's own devices could answer this: re-analyze existing captures with the LPP analyzers on and see whether any carrier LPP shows up. Would upgrade the honesty of the LPP pages from "never seen real traffic" to something measured, either way.
11. **`UPSTREAM.md` subscriber-identity entry** (§5.1): I can draft it — confirm and it lands alongside the docs work.
