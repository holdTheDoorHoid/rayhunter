# Working notes for Claude

## The documentation book

A full rewrite of `doc/` is underway. Before touching anything under `doc/`,
read `DOCS_PROMPT.md` (the phased plan — Phase 0 is done, its output is
`doc/INVENTORY.md` including the open questions) and `STYLE.md` (binding
writing standards). Prefer one page per session, committed separately with
the page name in the subject. Build with `mdbook build` from the repo root;
`create-missing = false` makes a bad `SUMMARY.md` link fail the build.
Decisions so far: quick-start targets the Orbic RC400L; the book lives on
this fork's `device-display-colors` branch (see `book.toml`'s edit URL).

## Toolchains (installed to home dir, no sudo)

- Node via nvm: `export NVM_DIR="$HOME/.nvm"; source "$NVM_DIR/nvm.sh"`
- Rust via rustup: `source "$HOME/.cargo/env"`
- `gh` CLI: `~/.local/bin/gh`

## Web UI dev loop (primary workflow)

The web UI is SvelteKit + TypeScript + Tailwind in `daemon/web`. Iterate against the
**live device** without building Rust or flashing anything:

```
cd daemon/web && API_TARGET=http://192.168.1.1:8080 npm run dev
```

Serves on http://localhost:5173 with hot reload; `/api/*` proxies to the real device.
With several Orbics around, point `API_TARGET` at a USB port-forward instead of an IP
(see "Talking to the right device") so the page is unambiguously one machine:

```
adb -s <serial> forward tcp:9090 tcp:8080
cd daemon/web && API_TARGET=http://localhost:9090 npm run dev
```

Before proposing changes upstream: `npm run lint`, `npm run check`, `npm test` in
`daemon/web`, plus `cargo fmt` and `cargo clippy` at the repo root.

## Building for the device

`rustup target add armv7-unknown-linux-musleabihf` is all the cross-compiler setup
needed — the `firmware-devel` profile links with `rust-lld`, so no external toolchain.

**Gotcha:** the daemon embeds the built web UI via `include_bytes!`, so
`npm run build` in `daemon/web` MUST run before `cargo build-daemon-firmware-devel`,
or the Rust build fails on missing `web/build/*` files. `make.sh` does both in order.

Deploying (`make.sh`) stops the service, `adb push`es the new binary to
`/data/rayhunter/rayhunter-daemon`, and reboots. This replaces one program file —
it is **not** a firmware flash, and is recoverable by pushing a working binary back.
It does require the device on **USB**; WiFi alone is not enough for adb.

Full install (needed when a device has no working setuid `rootshell`, so the
service cannot start as root):

```
./scripts/build-dev.sh build      # frontend + daemon + rootshell
adb kill-server                   # the installer needs exclusive USB access
./scripts/install-dev.sh orbic-usb
```

The installer refuses to run while an adb server holds the device — the error
says "being used by another program". Kill adb first, restart it afterwards.

**After pushing a new daemon, REBOOT — do not use `/etc/init.d/rayhunter_daemon start`.**
A daemon started from an adb-spawned shell fails to bind its port:

```
panicked at main.rs: Result::unwrap() on an Err value:
Os { code: 13, kind: PermissionDenied }   // TcpListener::bind
```

The port is genuinely free; the restriction comes from the context an adb-started
process inherits. Started by init at boot it binds fine. `/sbin/reboot` is denied
even under rootshell, so reboot with:

```
adb -s <serial> shell '/bin/rootshell -c "sync; echo b > /proc/sysrq-trigger"'
```

**The `sync` is not optional.** `echo b` is an immediate hard reset with no
filesystem flush, so a binary pushed moments earlier can be left truncated on
flash — it then starts and dies producing no log output at all, which looks
like a code bug and is not. Always verify before rebooting:

```
adb -s <serial> shell 'md5sum /data/rayhunter/rayhunter-daemon'
md5sum target/armv7-unknown-linux-musleabihf/firmware-devel/rayhunter-daemon
```

## Deploying: push via /tmp, not straight to /data

`adb push` to `/data/rayhunter/` **reports success while writing nothing**.
The directory is `root:root` mode 755 and the adb user is uid 2000, so it cannot
create files there. Push to `/tmp` and move it into place as root:

```
adb -s <serial> push <binary> /tmp/rayhunter-daemon.new
adb -s <serial> shell '/bin/rootshell -c "cat /tmp/rayhunter-daemon.new > /data/rayhunter/rayhunter-daemon && chmod 0777 /data/rayhunter/rayhunter-daemon && rm -f /tmp/rayhunter-daemon.new"'
```

Always compare md5 before rebooting. The checksum guard is what catches this.

## Radio measurements are intermittent, not continuous

ML1 serving cell and neighbour measurement reports (`0xb17f`, `0xb180`) only
arrive while the modem is doing something: attaching, reselecting, recovering.
An idle hotspot that is attached and stable **stops sending them entirely**, so
`/api/cell-info` legitimately freezes at the last reading for long stretches.

That is not a bug, and it wasted real time being mistaken for one. Before
suspecting the code, pull the capture and count what is actually in it:

```
adb -s <serial> pull /data/rayhunter/qmdl/<id>.qmdl.gz . && gunzip <id>.qmdl.gz
```

then parse with `Message::from_hdlc` over `split_inclusive(|&b| b == 0x7e)` and
count `LogBody` variants. Zero ML1 messages means the modem is quiet, not that
the daemon is broken. A **full power cycle** brings them back; a reboot may not.

## Building synthetic RRC messages

Deriving UPER bit layouts from the ASN.1 in `telcom-parser` works and is quick;
brute force searching the byte space does not and wasted hours. Read the
`#[asn(...)]` attributes for each type: CHOICE index widths come from the
alternative count, SEQUENCE preambles from `optional_fields`, extensible types
add a leading bit, and INTEGER widths from `lb`/`ub`. See `daemon/src/demo.rs`.

Two traps: PDU number **7** is DL-DCCH (6 is DL-CCCH, 2 is BCCH-DL-SCH), and the
V8 RRC packet has `earfcn` and `sib_mask` as **32 bit** fields.

**Analysis rows containing only informational events are never written** (see
`AnalysisRow::is_empty`), so a detector that only emits informational events can
never appear in the UI.

For a protocol whose ASN.1 is **not** in `telcom-parser` (LPP and RRLP were
two), derive the layout by hand but verify against an independent reference
encoder before trusting it — hand-derivation from memory of the spec missed the
extension bit on `LPP-TransactionID`, a one-bit error that shifted every field
after it, and mis-sized the `CommonIEsRequestLocationInformation` optional
bitmap (it is **7** root optionals, not 5). The reference bytes live as hex
constants in the Rust tests (`lib/src/analysis/lpp.rs`, `rrlp.rs`); the encoders
that produced them are offline and throwaway, not a dependency:
`pip install pycrate`, then `pycrate_asn1dir.LPP` / `.RRLP` for the ASN.1
payloads and `pycrate_mobile.TS44018_RR` for GSM RR framing (2G is TLV, not
ASN.1, so `pycrate_asn1dir` does not cover the transport). Only decode fields at
**fixed** offsets by hand — no variable-length content in front of them — and
stop at the first extension bit you cannot size.

**2G messages now reach analyzers.** `InformationElement::GSM` carries the raw
Layer 3 bytes (it was an empty stub), populated from `GsmtapType::Um` in
`information_element.rs`. The RRLP analyzer is the first consumer; any future 2G
heuristic builds on this. A demo `DemoMessage::Gsm` injects one through the real
diag→gsmtap→IE path, and the demo round-trip test is what proves the framing.

## Talking to the right device

Several Orbics may be connected at once, and they all serve on `192.168.1.1:8080` —
reachable over different interfaces simultaneously, so that address is ambiguous.
Never address a device by IP. Instead:

```
adb -s <serial> forward tcp:9090 tcp:8080   # then use http://localhost:9090
```

This pins every request to one physical unit over USB. When only an HTTP route is
available, fingerprint first: settings in `/data/rayhunter/config.toml` differ per
device. Deploying to the wrong unit is silent — they look identical over HTTP.

## Verifying display work on real hardware

The device's framebuffer can be read back, which turns "does it look right?" into a
measurement. 128x128, RGB565 little-endian, 32768 bytes:

```
adb -s <serial> exec-out '/bin/rootshell -c "dd if=/dev/fb0 bs=32768 count=1 2>/dev/null"' > fb.bin
```

Decode with `(v>>11)&0x1F <<3`, `(v>>5)&0x3F <<2`, `(v&0x1F)<<3`. Used this to measure
alert latency (a warning interrupted a 16s GIF in 0.19s) and exact bar heights (8/40/128
px requested gave 8/40/128 rows).

**Rayhunter draws over the device's own UI**, so the framebuffer also contains the
carrier's own interface. Counting "non-black rows" therefore over-counts badly. Set a distinctive
colour and count rows matching *that exact value* instead.

`POST /api/debug/display-state` with e.g. `{"WarningDetected":{"event_type":"High"}}`
forces a state without waiting for a real detection.

## What the display code actually does

Worth knowing before changing display settings, because it is easy to assume otherwise:

- The status line is drawn in **every** UI level except Invisible — as a thin line over
  the image in Demo and EFF logo, full-screen in High visibility, and as the fallback for
  states without a GIF in Custom GIF.
- **High visibility is not a separate feature.** It is the same status line at
  `fb.dimensions().height`. Same colour, same pattern, same code path.
- Severity is conveyed by line *pattern* as well as colour (dotted/dashed/solid), which
  is the colour-independent channel. Don't make patterns configurable.
- One-bit displays (TP-Link M7350) draw pixel-art faces and ignore colours entirely.

## Git remotes

`origin` = this fork. `upstream` = EFForg/rayhunter,
**push URL deliberately disabled**. Never push or open a PR to upstream without asking
the user first, every time.

## Upstream contribution norms

Per CONTRIBUTING.md: for anything beyond a small doc fix, check existing issues and talk
to the maintainers *before* implementing, or the contribution risks being rejected.

## Verifying web UI work: count requests, do not just look

Two traps, both of which cost real time.

**Svelte effects re-run for reasons unrelated to anyone clicking anything.**
An effect that acts every time it runs, and that writes state on the way
(assigning a fresh object counts), can feed itself and loop. This is invisible
on screen: the page looks right while it asks the device for the same thing
several times a second, which matters a lot on a single core that is also
recording. It also silently undoes anything the person did, a second or so
after they do it.

Drive such an effect from a **nonce the caller bumps once per click**, not from
the value being requested, and act only on a nonce not yet handled. See
`open_action` in `daemon/web/src/lib/packets.svelte.ts`. Keep the decision a
pure function: there is no component test setup here, and adding one means new
dev dependencies.

Check for this by counting, not by watching:

```js
const n = {}; const orig = window.fetch;
window.fetch = (...a) => { const u = `${a[0]}`; n[u] = (n[u]||0)+1; return orig(...a); };
// interact, wait, then read n
```

**The polling loop pauses when the tab is hidden** (`if (document.hidden) return;`
in `+page.svelte`). A browser pane that is collapsed or backgrounded therefore
sits at "Loading..." for ever with every endpoint healthy, which looks exactly
like a hang and is not one. Confirm with `document.visibilityState` before
debugging anything else. To test a hidden pane, override it:

```js
Object.defineProperty(Document.prototype, 'hidden', { configurable: true, get: () => false });
document.dispatchEvent(new Event('visibilitychange'));
```

## Deploying over a running daemon

`cat > /data/rayhunter/rayhunter-daemon` fails with **"Text file busy"** while the
service is running, and the `&&` chain after it silently skips the `sync`. Stop
the service first:

```
adb -s <serial> shell '/bin/rootshell -c "/etc/init.d/rayhunter_daemon stop"'
```

`chmod` on the pushed binary then fails with "Operation not permitted" because
the file is owned by uid 2000. That is harmless: `cat >` truncates in place and
keeps the existing 0777 mode. Check the mode and `sync` separately rather than
chaining, then verify md5 before rebooting.

## The framebuffer is shared, so anything drawn once gets half erased

Rayhunter does not own `/dev/fb0`. The device's own interface keeps redrawing
its parts of the screen, so **a picture painted a single time is partially
overwritten within seconds** and stays that way. This looked exactly like a
broken decoder: stable, partial, reproducible. Measured coverage was 1054 of
4096 sampled pixels, unchanged across samples.

Anything meant to stay on screen has to be repainted every pass, which is why
the status line always was. Decode once, cache the converted pixel buffer, and
write it each time round the loop.

## Image formats: convert, never assert

`DynamicImage::as_rgba8()` returns the buffer only when the image is *already*
in that layout, and `.unwrap()` on it panicked the display thread. A PNG saved
without transparency decodes as RGB, greyscale PNGs as Luma. GIFs always decode
to RGBA, which is why this held for as long as GIFs were the only input.

Use `.to_rgb8()`, which converts from anything. The panel has no alpha channel
anyway, so the fourth byte was being discarded a line later.

## Orbic display and power sysfs

```
/sys/devices/78b6000.spi/spi_master/spi1/spi1.0/sleep_mode   0 = blanked, 1 = awake
/sys/devices/78b6000.spi/spi_master/spi1/spi1.0/bl_gpio      backlight
/sys/power/autosleep                                         "mem" suspends, "off" does not
```

Writing the framebuffer does **not** count as activity to the blanking timer,
which is why the screen goes dark with Rayhunter plainly running. Write the
backlight before `sleep_mode` or a lit blank screen shows for an instant.

**For "is it plugged in", use `/sys/class/power_supply/usb/online`, not
`/sys/kernel/chg_info/chg_en`.** `chg_en` means "currently charging", so it
reads 0 on a device sitting on USB with a full battery, which is the desk setup
these features are for. Measured: chg_en 0, usb/online 1, on the same cable.
Note that `battery/orbic.rs` reads `chg_en` for `is_plugged_in`, so the battery
status shown in the web UI has this same quirk.

## Custom display images are stored per state as `<state>.gif`

The extension is historical; the file may hold a PNG. What it is gets decided
from its magic bytes at every use. Uploading a file only writes it to disk;
**the config's `display_gifs` entry for that state must also be set**, or the
display loop never loads it. That is why an upload can appear to do nothing.

## `POST /api/config` replaces the whole config

Fields left out of the body are reset to their **defaults**, not left alone.
Posting `{"min_space_to_start_recording_mb": 188}` silently turned off
`auto_delete_clean_recordings`, `ui_level` and everything else that had been
set. This wasted time twice.

To change one setting from the shell, read the config, edit it, post it back:

```
curl -s http://localhost:9090/api/config > cfg.json
python3 -c "import json;c=json.load(open('cfg.json'));c['ui_level']=5;json.dump(c,open('cfg.json','w'))"
curl -s -X POST http://localhost:9090/api/config -H 'Content-Type: application/json' --data-binary @cfg.json
```

Every config POST also **restarts the daemon**, which takes about 60 seconds
before the web UI answers again.

## One feature per commit

Upstream takes one feature per pull request, so commits here have to be
separable. **See `UPSTREAM.md`** for what the features on this branch are and
what each would need to stand alone.

From now on: one feature per commit, subject names the feature, and two
trailers so they can be found later.

```
Feature: keep-screen-on
Upstream: EFForg/rayhunter#916, EFForg/rayhunter#539
```

`Upstream: none` when no issue exists. Then:

```
git log --grep="^Feature: " --format="%h %s"      # every feature commit
git log --grep="^Feature: keep-screen-on"          # one feature's commits
```

A bug fix found while building a feature gets **its own commit and its own
slug**. Fixes are the easiest thing to get merged upstream and should not be
stuck behind the feature that uncovered them.

Commits earlier than `4f063d9` predate this and several bundle two or three
features. Do not cherry-pick those; `UPSTREAM.md` lists which files belong to
which feature so a clean branch can be built from `upstream/main` instead.

## `ServerState.config` is a startup snapshot, not live settings

It is cloned when the daemon starts and never updated. Anything the API can
change during a run therefore cannot be read back from it, and writing the
config file from it silently reverts whatever changed since boot.

This produced two bugs at once with web accounts: a new account vanished from
the settings page on the next reload, and was then erased from `config.toml`
by the next save of any setting at all, because `set_config` rebuilt the file
from the snapshot. Anything mutable at runtime belongs in its own lock on
`ServerState` — see `web_users` — with every read site pointed at it. Grep for
other `state.config.<field>` reads before adding a new settable field.

## A top-level key appended to `config.toml` lands inside `[analyzers]`

`dist/config.toml.in` ends with the `[analyzers]` table, so
`config.push_str("\nterminal_enabled = true\n")` is parsed as
`analyzers.terminal_enabled`. `AnalyzerConfig` is `#[serde(default)]` without
`deny_unknown_fields`, so the key is dropped with no error and the setting
simply never takes effect. This is why `--enable-terminal` did nothing for as
long as it existed.

Top-level keys go **above the first table header**. Use
`set_top_level_bool` in `installer/src/connection.rs`. Check placement by
parsing the result, not by reading the file:

```python
import tomllib; tomllib.loads(text)["terminal_enabled"]
```

## One diag record can be several GSMTAP frames

A MAC transport block record holds a list of blocks, not one. Measured on an
Orbic: downlink records average two and reach ten, uplink reach twenty three.
A parser that returns one frame per record silently drops most of that
traffic, and the capture still looks plausible.

`gsmtap::parser::parse_all` returns all of them and is what writes the PCAP.
`parse` returns the first and is what the **analysis** side uses, deliberately:
analysis counts one row per diag message, and `packet_num` in both the
analyzer and the packet explorer means "the Nth diag message". Moving analysis
to `parse_all` would renumber packets and break jump-to-packet. The capture has
never been one frame per diag message anyway, since records that produce
nothing are skipped entirely.

`log_to_gsmtap` still has no arm for MAC DL/UL, so `parse` returns None for
them and the analysis side is untouched by their arrival.

## Qualcomm and Wireshark number RNTI types differently

Qualcomm's 0 is a C-RNTI, which is **3** to Wireshark. Copying the byte
through labels every ordinary transmission as something else, and nobody
reading the capture can tell. See `wireshark_rnti_type` in
`lib/src/gsmtap/mac.rs`. The constants are in SCAT's `util.py`, not guessed.

**Transport blocks and random access responses are both MAC-LTE frames**, and
a transport block is long enough to parse as a response full of plausible
nonsense. That would feed invented timing advances to the detector that reads
timing advance. They are told apart by the tag after the three context bytes
(0x01 payload tag means a response, 0x04 frame/subframe tag means a transport
block) and by the RNTI type.

## Check the API docs build before tagging a release

The release pipeline builds the OpenAPI spec with a feature nothing else uses,
so code that compiles and passes every test can still fail the release. This is
what broke the v0.12.1 attempt: `chrono::DateTime<Local>` has no `ToSchema`
impl, so any new API type with a timestamp field needs the annotation the
manifest already uses:

```rust
#[cfg_attr(feature = "apidocs", schema(value_type = String))]
pub last_seen: DateTime<Local>,
```

Run exactly what CI runs, from the repo root:

```bash
cargo run --bin gen_api --features apidocs -- /tmp/openapi.json
```

**Do not use `-p rayhunter-daemon --features apidocs`.** That skips workspace
feature unification, so `rayhunter/apidocs` never turns on and you get a dozen
errors about `Device`, `AnalyzerConfig` and `EventType` that do not exist in
CI. They are an artifact of the invocation, not real.

A new endpoint also has to be listed in the `paths(...)` block of `ApiDocs` in
`daemon/src/lib.rs`, and its request and response types declared with
`request_body(content = ...)` and `body = ...`, or it compiles fine and is
silently missing from the published docs.
