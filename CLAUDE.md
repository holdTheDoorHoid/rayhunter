# Working notes for Claude

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
