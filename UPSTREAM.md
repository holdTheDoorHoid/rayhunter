# Features on this branch, and how to propose them upstream

This fork carries a lot of separate work. Upstream takes one feature per pull
request, so this file exists to say what the separate things actually are, and
what each one would need to stand on its own.

Read `CONTRIBUTING.md` in this repo first. For anything beyond a small
documentation fix it asks you to check existing issues and **talk to the
maintainers before implementing**, or the contribution risks being rejected.
That has already happened to at least one feature listed here.

`origin` is this fork. `upstream` is EFForg/rayhunter and its push URL is
deliberately disabled. Never push or open a pull request there without asking
first, every time.

## How to build a single feature pull request

Do not cherry-pick from this branch. Several commits carry more than one
feature, because they were written before this file existed. Instead:

```
git fetch upstream
git checkout -b feature-name upstream/main
```

then bring across only the files and hunks listed for that feature below, using
this branch as the reference. `git diff upstream/main -- <path>` shows
everything this branch changed in a file; take only the parts that belong to
the feature you are submitting.

## The convention from here on

One feature per commit. The subject names the feature. Two trailers make them
greppable later:

```
Feature: keep-screen-on
Upstream: EFForg/rayhunter#916, EFForg/rayhunter#539
```

`Upstream: none` when there is no issue for it yet. `git log --grep="^Feature: "`
then lists them, and `git log --grep="^Feature: keep-screen-on"` finds every
commit belonging to one feature even when it took several.

A bug fix that stands alone gets its own commit and its own `Feature:` slug, so
it can be offered upstream separately. Fixes are the easiest thing to get
merged and should not be held hostage to the feature that uncovered them.

---

## Ready to propose

These have a clear upstream issue, and in each case a maintainer has said
something encouraging on it.

### severity-counts
Warning counts broken out per severity instead of one total, in the history and
current recording panels.
**Upstream:** [#363](https://github.com/EFForg/rayhunter/issues/363). A
maintainer asked for exactly this on the issue: sum each level and show a count
for each.
**Commits:** `627a288` (part).
**Files:** `analysis.svelte.ts` (the `by_severity` field and its computation),
`analysis.svelte.spec.ts`, `components/AnalysisStatus.svelte`.
**Depends on:** nothing.

### recording-names-and-notes
A display name and free text notes per recording, shown in the web UI and used
for the downloaded zip filename.
**Upstream:** [#501](https://github.com/EFForg/rayhunter/issues/501), which was
opened by a maintainer.
**Commits:** `5500691` (part), `4f063d9` (the history table half).
**Files:** `qmdl_store.rs` (the two manifest fields, `set_entry_annotations`,
`sanitize_display_name`, its tests), `server.rs` (`annotate_recording`, the zip
`Content-Disposition`), `main.rs` (the route), `manifest.svelte.ts`,
`utils.svelte.ts` (`annotate_recording`), `components/RecordingNotes.svelte`,
`components/ManifestCard.svelte`, `components/ManifestTableRow.svelte`.
**Depends on:** nothing.
**Note:** the issue asks for the name to be constrained to `\w{0-29}`, which is
what `sanitize_display_name` does. Keep its tests: the name reaches a filename
and an HTTP header, and they cover path traversal and header injection.

### custom-display-images
A user-uploaded image per display state, GIF played as animation and PNG shown
as a still.
**Upstream:** [#914](https://github.com/EFForg/rayhunter/issues/914). A
maintainer said custom images seem reasonable but that nobody's art should ship
in the release. Nothing is bundled here, so that holds.
**Commits:** `206e24a`, `4945032` (preview), `0b4d67e` (size limits), `0a67632`
(the PNG half).
**Files:** `config.rs` (`DisplayGifs`, `gif_store_path`), `server.rs` (upload,
serve, delete), `display/generic_framebuffer.rs` (`image_kind`,
`image_dimensions`, `decode_still_image`, the CustomGif branch),
`components/DeviceGifSettings.svelte`, `utils.svelte.ts`.
**Depends on:** nothing, but see `image-format-crash-fix`, which this needs.

### legacy-radio-in-pcap
2G and 3G signalling written to the PCAP instead of dropped, and traffic
Rayhunter never analyses no longer listed as a parse failure.
**Upstream:** [#1013](https://github.com/EFForg/rayhunter/issues/1013), opened
by a maintainer, plus the noise half of
[#457](https://github.com/EFForg/rayhunter/issues/457).
**Commits:** `e913690`.
**Files:** `lib/src/gsmtap/parser.rs` (`gsm_rr_subtype` and the four new log
body arms), `lib/src/analysis/analyzer.rs` (`is_deliberately_unanalysed` and
its two call sites).
**Depends on:** nothing.
**Note:** the 2G and 3G paths are unit tested but were not exercised end to
end, because the test device only sees LTE. Anyone with 2G or 3G service
should confirm before this goes up.

## Already done upstream, nothing to send

### check-json-output
**Upstream:** [#570](https://github.com/EFForg/rayhunter/issues/570). Another
contributor claimed it on the issue and
[PR #1088](https://github.com/EFForg/rayhunter/pull/1088) is **merged**. Our
fork already has it, since we branched after it landed: `rayhunter-check -j`.
The only part of the issue arguably unmet is writing one JSON file per input
when `-p` is given a directory, rather than a single output file.

## Claimed by somebody else

Check before starting. Duplicating claimed work wastes it and is unlikely to
be merged.

### neighbouring-cells (#326)
`simonft` said on the issue that they are working on it, with a maintainer's
encouragement. Most of what the issue asks for already exists in this fork
under `cell-site-panel`. The parts it asks for that we do **not** have are an
OpenCellID lookup or map link, and showing IMSI, TMSI and IMEI.

### layer-2-mac (#457)
A maintainer has a work in progress branch, `fix-457`, and laid out a three
step plan on the issue. Step one, enabling LTE MAC logging, is already in this
fork: `LOG_LTE_MAC_DL` and `LOG_LTE_MAC_UL` are in
`LOG_CODES_FOR_RAW_PACKET_LOGGING`, and `lib/src/gsmtap/mac.rs` already builds
GSMTAP for RACH responses. Steps two and three, writing MAC DL and UL to the
PCAP and parsing them for analyzers, are not done. Timing advance is the prize
there: it gives distance to the tower.

### lpp-heuristic (#1072)
Two analyzers for LPP, the LTE Positioning Protocol (the network asking the
device to measure and report its own position), separately toggleable:

- `lpp_location_request` (basic): a location request and the device's report
  each raise a low warning once per LPP transaction; capability exchanges and
  GPS assistance data are informational. Reads only the message prefix.
- `lpp_location_tracking` (deep): decodes the request/response bodies to report
  the positioning method (A-GNSS satellite, OTDOA timing, E-CID cell) and
  whether the request is for **periodic** (continuous) reporting, which it
  raises to **medium**. Split out as its own analyzer so a memory-constrained
  device can disable it and keep the basic one — the user asked for exactly
  this. Reads more of each message, all at fixed offsets.
**Upstream:** [#1072](https://github.com/EFForg/rayhunter/issues/1072), opened
by a maintainer, also touching
[#534](https://github.com/EFForg/rayhunter/issues/534).
**Claimed:** `philgebhardt` said on the issue they would like to build the
larger version: full LPP ASN.1 definitions in `telcom-parser`, pairing request
to response by transaction ID. A maintainer replied with a question and no code
has appeared yet. This fork's version reads only the fixed UPER prefix of the
message (presence bits, transaction ID, message kind), which needs no new
ASN.1 and still pairs request with response — but it is a different approach
from the one claimed. **Coordinate on the issue before proposing this**, so it
lands as a complement or a starting point rather than a duplication.
**Files:** `lib/src/analysis/lpp.rs`, `lib/src/analysis/mod.rs`,
`lib/src/analysis/analyzer.rs` (config field and harness wiring),
`dist/config.toml.in`, `daemon/web/src/lib/utils.svelte.ts`,
`daemon/web/src/lib/heuristics.ts`, `doc/heuristics.md`, `daemon/src/demo.rs`
(the LPP scenario and the relaxed per-scenario severity test).
**Depends on:** nothing.
**Note:** the issue asks for an *informational* heuristic, but rows carrying
only informational events are never written (see `AnalysisRow::is_empty`), so
a purely informational analyzer would be invisible. The two location-moving
messages therefore warn at Low, once per transaction; everything else stays
informational. Say so when proposing, since upstream may prefer to change the
row-writing rule instead. The test vectors in `lpp.rs` were generated with
pycrate's reference 36.355 encoder and caught a real one-bit layout mistake;
keep them. The deep analyzer's method/periodic offsets are verified against the
same encoder — do not "simplify" them without re-checking against vectors.

### rrlp-heuristic (#534)
An analyzer for RRLP (Radio Resource LCS Protocol, 3GPP TS 44.031), the 2G
counterpart to LPP: the GSM network asking a handset to report its position. A
location request (`msrPositionReq`) or the device's response (`msrPositionRsp`)
warns at low; assistance data and errors are informational.
**Upstream:** [#534](https://github.com/EFForg/rayhunter/issues/534), which asks
for both RRLP and LPP location-usage detection and is **unclaimed**. Still,
#534 is a large multi-part wishlist issue (silent SMS, empty paging, LAC
changes, and more); propose this as "the RRLP location part of #534", not as
closing it.
**Files:** `lib/src/analysis/rrlp.rs`, `lib/src/analysis/mod.rs`,
`lib/src/analysis/analyzer.rs` (config field and wiring),
`lib/src/analysis/information_element.rs` (the `GSM` variant now carries the
raw L3 bytes, and `TryFrom` populates it from `GsmtapType::Um`),
`daemon/src/packet_explorer.rs` (the `GSM` match arm),
`dist/config.toml.in`, `daemon/web/src/lib/utils.svelte.ts`,
`daemon/web/src/lib/heuristics.ts`, `doc/heuristics.md`, `daemon/src/demo.rs`
(the `Gsm` demo message kind, `encapsulate_gsm`, and the RRLP scenario).
**Depends on:** the `InformationElement::GSM`-carries-bytes change is the
reusable half — it is the first time 2G messages reach an analyzer at all, so
any future 2G heuristic builds on it. Worth calling out separately upstream.
**Note:** the transport framing (GSM RR APPLICATION INFORMATION, message type
0x38) is verified against `pycrate_mobile`'s TS 44.018 encoder and the RRLP
APDU against pycrate's 44.031, both in `rrlp.rs`'s tests, and the whole
diag→gsmtap→analyzer path is exercised by the demo round-trip test. It has
**not** been seen against a real 2G RRLP capture (the test devices are LTE), so
say that when proposing. Detection requires both a valid APPLICATION
INFORMATION header and a decodable RRLP APDU, so a wrong message-type guess
cannot produce a false positive on its own.

### web-authentication
Optional accounts for the web interface, off by default so an update cannot
lock anyone out.
**Upstream:** none. The interface has never had any authentication, and on a
hotspot that means anyone on the WiFi can read the recordings, so this is
likely to interest upstream regardless of the rest of this fork.
**Commits:** `30b4ce1`, `d6e4b26`.
**Files:** `web_auth.rs`, `config.rs` (`web_users`), `main.rs` (the middleware
layer, two routes, and seeding `ServerState.web_users`), `server.rs` (hash
redaction, account preservation on save, `set_web_user`, `delete_web_user`,
`write_web_users`), `components/ConfigForm.svelte`, `utils.svelte.ts`.
**Depends on:** nothing.
**Note:** there is no TLS on these devices, so this is a second factor beyond
the WiFi password rather than a secure channel, and the interface says so. Keep
the published test vectors: an unverified key derivation still looks like it
works. Accounts live in `ServerState.web_users`, not in `ServerState.config`,
which is a startup snapshot: reading them from the snapshot made a new account
vanish on the next reload and then be erased by the next settings save.

### web-terminal
Run one command on the device from the interface. Enabled only by the
installer's `--enable-terminal` flag, never from the interface itself.
**Upstream:** none. Would need discussing before proposing: it gives root
command execution to an interface that upstream still ships without
authentication.
**Commits:** `f76734f`, `6db029f`.
**Files:** `server.rs` (`run_terminal_command` and its types), `config.rs`
(`terminal_enabled`), `main.rs` (the route),
`components/Terminal.svelte`, `routes/+page.svelte`, `utils.svelte.ts`,
`installer/src/connection.rs`, `installer/src/lib.rs`,
`installer/src/orbic_network.rs`, `installer/src/moxee.rs`.
**Depends on:** `web-authentication` in practice, though not in code. Proposing
the terminal without a way to put a password on the interface would be hard to
justify.
**Note:** the flag writes `terminal_enabled` above the first table header in
`config.toml`, never at the end. The template finishes inside `[analyzers]`, so
a key appended to it is read as `analyzers.terminal_enabled` and dropped in
silence. `set_top_level_bool` and its tests exist for that reason.

## Bug fixes worth offering on their own

Each of these is small, self-contained, and fixes something that affects
upstream today. They are the easiest things to get merged.

### image-format-crash-fix
`DynamicImage::as_rgba8().unwrap()` in the framebuffer panicked the display
thread for any image that was not already RGBA, which includes a PNG saved
without transparency. Uses `to_rgb8()` instead.
**Upstream:** none. Affects upstream only once PNGs are accepted, but the
unwrap is wrong today regardless.
**Commits:** `0a67632` (part).
**Files:** `display/generic_framebuffer.rs`, `write_dynamic_image` and its test.

### still-image-repaint
Rayhunter does not own the framebuffer; the device's own interface redraws over
it. Anything drawn once ends up half erased. Still images are decoded once and
repainted every pass.
**Upstream:** none.
**Commits:** `0a67632` (part).
**Files:** `display/generic_framebuffer.rs`.
**Depends on:** `custom-display-images`.

### not-this-device-error
A response that is a web page rather than data is reported as what it is,
instead of as a JSON parser error. Happens whenever the request never reaches
the daemon: a phone on mobile data rather than the device's WiFi, a VPN, a
captive portal, or a different device at that address.
**Upstream:** none. Affects upstream identically.
**Commits:** `d3fc0d3`.
**Files:** `utils.svelte.ts` (`looks_like_a_web_page`, `NOT_THE_DEVICE_MESSAGE`,
`req_json`, `get_logs`), `analysisManager.svelte.ts`, `notThisDevice.spec.ts`.

### gps-page-load
`/api/gps` returns 404 when GPS is off, which is the normal configuration, and
that 404 was thrown before `loaded` was set, leaving the whole page on
"Loading..." with every other panel ready.
**Upstream:** none found. Affects anyone running with GPS disabled.
**Commits:** `516aa5e`.
**Files:** `routes/+page.svelte`.

### explorer-refetch-loop
The packet explorer refetched the same window several times a second forever,
because a Svelte effect acted on every run rather than on each request.
**Upstream:** none. Only affects this fork, since the explorer is ours.
**Commits:** `5153763` (part).
**Files:** `packets.svelte.ts` (`open_action`), `components/PacketExplorer.svelte`,
`components/AnalysisView.svelte`.
**Depends on:** `packet-explorer`.

### modal-close-dark-mode
The close button had a hardcoded dark fill and vanished against a dark
background, with no other way out of the dialog.
**Upstream:** none, but it lands with `dark-mode`.
**Commits:** `729b694`.
**Files:** `components/Modal.svelte`.

## Proposed upstream before and closed

### keep-screen-on
Stops the device blanking its screen on its own timer. Three states: never,
only while plugged in, always.
**Upstream:** [#916](https://github.com/EFForg/rayhunter/issues/916) and
[#539](https://github.com/EFForg/rayhunter/issues/539).
[PR #919](https://github.com/EFForg/rayhunter/pull/919) attempted this and
**was closed**. The maintainer's objections were: too much code for a minor
feature, Orbic only so no feature parity, and that left on it would flatten the
battery.
**What is different here:** the third objection is answered directly by the
"only while plugged in" state, which is also what #539 asked for. Auto-suspend
is restored the moment it stops holding the screen rather than only at
shutdown. It shuts down promptly instead of after a full poll interval.
The first two objections still stand and would need discussing before any new
attempt.
**Commits:** `dfbb67d` (part).
**Files:** `config.rs` (`KeepScreenOn`), `display/orbic.rs`,
`components/ConfigForm.svelte`, `utils.svelte.ts`.
**Note:** the plugged-in test reads `/sys/class/power_supply/usb/online`, not
`chg_info/chg_en`. `chg_en` means "currently charging" and reads 0 on a device
on USB with a full battery. `battery/orbic.rs` upstream uses `chg_en` for
`is_plugged_in` and has this same quirk, which may be worth reporting on its
own.

## No upstream issue yet, worth opening one first

Per `CONTRIBUTING.md`, ask before building on these further.

### packet-explorer
Browse the messages in a recording from the web UI, decoded by the same path
the detectors use.
**Commits:** `ec061f7`, `d838b36`, `69b9e51`, `5153763` (part).
**Files:** `packet_explorer.rs`, `server.rs` (the two routes), `main.rs`,
`analysis/analyzer.rs` (`packet_num` on rows, report version 3),
`packets.svelte.ts`, `components/PacketExplorer.svelte`,
`components/PacketDetail.svelte`.
**Note:** carries a report format bump to version 3. That alone needs
discussing upstream.

### warning-markers-and-jump
Severity badges on packets that raised a warning, a jump to packet number box,
and alert packets exempt from the filters.
**Commits:** `5153763`, `627a288` (part).
**Files:** `components/PacketExplorer.svelte`, `components/AnalysisView.svelte`,
`packets.svelte.ts` (`apply_filters` gains `alwaysKeep`), `packets.spec.ts`.
**Depends on:** `packet-explorer`.

### recording-rotation
Start a new recording automatically after a size or a length of time.
**Commits:** `3903c86`.
**Files:** `config.rs`, `diag.rs` (`rotate`, `finish_current_entry`,
`rotation_bytes`, `rotation_duration`), `main.rs`, `recordingRotation.ts`,
`recordingRotation.spec.ts`, `components/ConfigForm.svelte`, `utils.svelte.ts`.

### auto-delete-clean-recordings
Remove recordings that found nothing when space runs low, oldest first.
**Commits:** `5500691` (part).
**Files:** `cleanup.rs`, `config.rs`, `diag.rs` (the two prune calls),
`lib.rs`, `main.rs`, `components/ConfigForm.svelte`, `utils.svelte.ts`.
**Depends on:** `recording-names-and-notes`, because a named recording is
protected from deletion. Droppable if submitted alone.

### button-pauses-the-overlay
A button press shrinks Rayhunter to its thin status line for twenty seconds, so
the device's own screens including the wifi password can be read.
**Commits:** `0a67632` (part).
**Files:** `display/mod.rs` (`DisplaySuppression`), `key_input.rs`,
`display/generic_framebuffer.rs`, every `display/*.rs` for the signature,
`config.rs`, `main.rs`, `components/ConfigForm.svelte`.
**Note:** touches every device display module because they share one signature.
That is most of its diff and worth flagging in any pull request.

### explanations-toggle
A setting that hides every expandable explanation in the interface.
**Commits:** `e15fa7f`, `627a288` (part), `dfbb67d` (the cell site half).
**Files:** `helpVisibility.svelte.ts`, `helpVisibility.spec.ts`,
`components/Explainer.svelte`, `routes/+layout.svelte`,
`components/ConfigForm.svelte`, `components/CellInfoView.svelte`.
**Depends on:** whichever explanation-carrying features go up with it.

### config-page-sections
The configuration page as five tabs rather than one long scroll.
**Commits:** `4f063d9` (the ConfigForm half).
**Files:** `components/ConfigForm.svelte`.
**Note:** the largest diff on the branch and almost entirely a move of existing
markup. Would be far easier to review if submitted after the features that add
settings to that page, not before.

### cell-site-panel
Serving cell, neighbours, radio details, encryption in use, detection health,
tracking area changes.
**Commits:** `1b38918`, `bddc55b`, `fb84cba`, `8e7719d`, `d1d8db2`, `48c6573`.
**Files:** `cell_info.rs`, `diag.rs` (`update_cell_info`), `server.rs`,
`cellInfo.ts`, `lteBands.ts`, `components/CellInfoView.svelte`.

### demo-control
A config-gated button that injects a clearly labelled synthetic warning through
the real detectors.
**Commits:** `3302b0c`, `73ce4e8`, `64258c6`, `a327f02`, `28b2a97`.
**Files:** `demo.rs`, `config.rs`, `diag.rs`, `server.rs`, `main.rs`,
`components/DemoButton.svelte`, `components/ConfigForm.svelte`.

### dark-mode
A dark appearance for the web UI.
**Commits:** `06188ff`, `729b694`.
**Files:** `theme.svelte.ts`, `app.css`, and a dark variant on nearly every
component.
**Note:** touches almost every component, so it wants to go up early or not at
all. Rebasing it after other UI work is painful.

### display-colours, status-line-height
Per-state colour overrides and a configurable status line height.
**Commits:** `5b6770f`, `f6c995a`.
**Files:** `config.rs` (`DisplayColors`, `status_bar_height`),
`display/generic_framebuffer.rs`, `components/DeviceColorSettings.svelte`,
`colorAdvice.ts`.

### heuristic-explanations
Plain language descriptions of what each detector looks for.
**Commits:** `5355783`.
**Files:** `heuristics.ts`, `components/ConfigForm.svelte`.

### system-information
Load, uptime, temperature, recording headroom, and measured processor usage
rather than load average.
**Commits:** `3fd661e`, `cf197c7`, `627a288` (the capitalisation half).
**Files:** `stats.rs`, `systemStats.ts`, `systemStats.spec.ts`,
`components/SystemStatsTable.svelte`.

## Not for upstream

`CLAUDE.md` and this file are working notes for this fork. `40a7d49`,
`4945032`, `0735392`, `1257791`, `6bcbfbd` are documentation only.
