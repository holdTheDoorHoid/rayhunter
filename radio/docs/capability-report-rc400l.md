# Phase 0: radio capability of the Orbic RC400L

Measured on a live RC400L (USB serial `49514baf`) on 2026-08-31/09-01, against
this fork at `c3cba9b`. Every claim below is either a command's output or a
string extracted from the shipped driver. Where something is inferred rather
than observed, it says so.

## Summary

| Question | Answer |
|---|---|
| Wi-Fi chipset | **QCA9377** (`HW:QCA93x7_REV1_1`), SDIO, 1×1, dual-band |
| Driver | qcacld-2.0, `Host SW:4.0.11.205G`, `FW:0.0.0.9` |
| Monitor mode | **Compiled in but not reachable.** No `con_mode` exposes it |
| BSS scanning | **Works, both bands, without disturbing the hotspot** |
| Probe-request capture | **Not possible** on this hardware |
| Bluetooth silicon | **Present on the die** (QCA9377 is a combo part) |
| Bluetooth usable | **No.** `CONFIG_BT` unset, no UART, no device-tree node |
| Privileged execution | Only from init or `AT+SYSCMD`; **not** from adb |

The headline for planning: **Phase 1 is fully viable, Phase 2 is blocked on
this hardware, and Phase 4 needs a companion radio** — not because the chip
lacks a Bluetooth radio, but because nothing in the OS can reach it.

## 1. What the radio is

```
cnss_sdio:cnss_sdio_wlan_inserted:836:: SDIO Device is Probed
wlan: loading driver v4.0.11.205G
ol_download_firmware: chip_id:0x5020001 board_id:0x0
Host SW:4.0.11.205G, FW:0.0.0.9, HW:QCA93x7_REV1_1
```

SDIO identity `0271:0701`, driver at `/usr/lib/modules/3.18.48/extra/wlan.ko`,
built from `qcacld-2.0` (the build path in the binary names
`mdm9607-oe-linux-gnueabi/qcacld-hl` — `hl` is the high-latency, i.e. SDIO,
variant). SoC is an MDM9207 on a `qcom,mdm9607-mtp` board, kernel 3.18.48,
160 MB RAM.

**Portability caveat.** `/etc/init.d/load_wifi_driver.sh` selects a driver by
SDIO ID and can load `rtl8192es` or `rtl8189es` instead:

```sh
if [ $value == "0x818b" ] ... rtl8192es.ko
elif [ $value == "0x8179" ] ... rtl8189es.ko
elif [ $value == "0x050a" ] ... wlan.ko   # labelled "qca6174"
```

So **the RC400L ships with more than one Wi-Fi chip depending on production
run.** Nothing here may assume a QCA9377 without checking. Any capability
detection must be done at runtime on the actual unit.

## 2. Interface modes and what scanning can see

`iw list` advertises `IBSS, managed, AP, P2P-client, P2P-GO`. **Monitor is
absent.** The valid interface combinations include:

```
 * #{ managed } <= 2, #{ AP } <= 2, total <= 4, #channels <= 2
```

which is what makes Phase 1 work: a managed interface can be added alongside
the two AP interfaces and scanned on, without touching the hotspot.

Measured, with the access point live throughout:

```
scan 1 rc=0 bss=14 hostapd=1
scan 2 rc=0 bss=15 hostapd=1
scan 3 rc=0 bss=16 hostapd=1
```

Roughly 4.5 s per full scan. Frequencies observed spanned both bands —
2417/2432/2437/2462 and 5280/5500/5745/5785 MHz — so **channel coverage is not
the problem cooperq suspected in PR #1042**; a dedicated managed interface
sweeps both bands. Scan records carry BSSID, SSID, frequency, signal in dBm,
and decoded information elements (RSN, HT, WPS, Country).

What a BSS scan cannot see is the important part: **it returns beacons and
probe responses from access points.** It never returns probe requests. Current
Flock cameras, per the flock-you research, no longer run a management access
point and instead emit wildcard probe requests. **Such a device is invisible to
this capture method no matter how good the signature is**, and a non-detection
must never be presented as an all-clear.

## 3. Monitor mode: present in the driver, unreachable in practice

The driver contains a complete monitor implementation. Symbols include
`hdd_mon_open`, `__hdd_mon_open`, `wlan_mon_drv_ops`, `mon_mode_ether_setup`,
`sme_create_mon_session`, `lim_mon_init_session`, `ol_rx_mon_indication_handler`,
`hdd_mon_rx_packet_cbk`, and — notably for our bus — `htt_rx_mon_amsdu_pop_hl`,
the high-latency (SDIO) monitor receive path. There is an `iwpriv` command
`setMonChan` and the strings `"Monitor mode is enabled"` and
`"Not supported, device is not in monitor mode"`.

The vendor's own init script confirms `con_mode` is the qcacld-2.0
`tVOS_CON_MODE` enum:

```sh
start_ftm)  do_ctrl_cld_ll start con_mode=5     # VOS_FTM_MODE = 5
```

and the running device reports `con_mode=1` while serving SoftAP
(`VOS_STA_SAP_MODE = 1`). Both match.

**Every value from 0 to 14 was tested by reloading the driver.** All of them
produced a single `wlan0` of `type managed`, monitor never appeared in
`iw list`, and no extra netdev was created:

```
==================== con_mode=12
insmod rc=0   readback=12   type managed   monitor-in-modes=0
```

Also tested and rejected:

- `iw dev wlan0 interface add mon0 type monitor` → `Operation not supported (-95)`
- `iw dev <vif> set type monitor` → `-95`
- `iw dev <vif> set monitor control` → `-95`
- `iwpriv wlan0 setMonChan 6` → `Invalid command` (the command is registered
  only on an adapter already in monitor device mode, which nothing creates)

Conclusion: the wiphy never advertises `NL80211_IFTYPE_MONITOR`, so this is a
compile-time gate in the vendor's build, not a runtime setting. Reaching
monitor mode would need a rebuilt `wlan.ko` against qcacld-2.0 sources matching
this kernel — out of scope, and explicitly not worth risking device stability
for.

**Unresolved lead.** `ol_pktlog_init: pktlogmod_init successfull` appears at
boot, `/proc/ath_pktlog/cld` and `/proc/sys/ath_pktlog/cld/` exist, and the
driver exports `ath_sysctl_pktlog_enable`. Qualcomm's pktlog carries RX
descriptors. Whether it can yield foreign management frames — as opposed to
per-VAP debug statistics — was not determined and is the one avenue left for
frame-level visibility without replacing the driver. It is not a V1
dependency.

## 4. Bluetooth: the silicon is there, the software is not

This was worth chasing, and the answer is genuinely mixed.

**In favour of BT existing:**

- The QCA9377 is a **combo WLAN + Bluetooth part**; the BT radio is on the same die.
- `/firmware/image/btfw32.tlv` (65 KB) and `/firmware/image/btnv32.bin` (2 KB) —
  the ROME 3.2 Bluetooth firmware and NVM pair matching this chip generation.
- `/usr/bin/hci_qcomm_init` is present.
- The WLAN configuration enables coexistence arbitration: `gCoexPtaConfigEnable=1`.

**Against it being usable:**

- `# CONFIG_BT is not set` in `/proc/config.gz`. There is **no Bluetooth stack
  in the kernel at all** — no HCI, no BlueZ, no `/sys/class/bluetooth`.
- `/sys/class/rfkill/*/type` lists only `wlan`.
- No Bluetooth node anywhere in `/proc/device-tree`; no `bt_en` GPIO, no BT
  regulator, no BT SMD channel.
- The only enabled UART is the console (`serial@78b3000`). The other two
  (`serial@78b0000`, `uart@78b1000`) are `status = disabled`, and
  `msm_serial_hs` has no bound devices — so there is no port for a BT
  controller to be attached to.
- `hci_qcomm_init` is the legacy "Bahama"-era QSoC tool (its strings reference
  `R2B`/`R2C`, `Poke Bahama B0`, `libbtnv.so.0`), not the ROME bring-up path.
  It looks like generic BSP residue rather than evidence of a wired radio.
- Only one SDIO function (`mmc0:0001:1`) is present, so BT is not on SDIO.

**Verdict.** The hardware very likely has a Bluetooth radio, and the user's
hunch was right about the silicon. But reaching it would require, at minimum,
a kernel rebuilt with `CONFIG_BT`, a device-tree change to enable a UART and
declare a BT node, and confirmation that the RC400L's PCB actually routes the
BT UART and enable lines — which **cannot be established from software** and
would need a board teardown. Editing the boot device tree is also the one class
of change that is not recoverable over USB.

So BLE is correctly treated as capability-gated and absent on this device. The
[EFF maintainers' statement in issue #1000](https://github.com/EFForg/rayhunter/issues/1000)
that "this device doesn't support bluetooth" is right in every way that
matters operationally, though the reason is the software image rather than the
silicon.

## 5. The capability constraint that shapes the architecture

This is the most consequential discovery, and it is not about radios.

```
adbd            CapBnd: 00000000000000c0    (CAP_SETUID | CAP_SETGID only)
init            CapBnd: 0000003fffffffff    (full)
rayhunter-daemon CapBnd: 0000003fffffffff   (full)
```

**Everything launched from adb inherits a crippled capability bounding set.**
Since the bounding set can never be widened, no adb-spawned process — even as
uid 0 via `rootshell` — can load a kernel module or configure a network
interface:

```
rmmod wlan       -> Operation not permitted
insmod wlan.ko   -> Operation not permitted
ifconfig wlan0 up -> socket: Permission denied
```

Three consequences:

1. **The radio daemon must be started by init**, exactly like
   `rayhunter-daemon`, or it cannot create its scan interface. It will then
   have the capabilities it needs.
2. **Hardware experiments must run via `AT+SYSCMD`** (the installer's
   `adb_at_syscmd`, reachable as `installer util serial 'AT+SYSCMD=…'`), which
   is handled by an init-started daemon and therefore runs with
   `CapEff: 0000003fffffffff`. This is what made testing possible without
   reboot cycles.
3. **Blast radius is smaller than it looks.** Because adb cannot unload the
   Wi-Fi driver, ordinary development cannot break the radio. The failure
   actually hit during this work was killing `hostapd` and then being unable to
   restart it (that needs `CAP_NET_ADMIN`); a reboot restored everything.

`/etc` is owned by uid 1000, not root, so writing an init script also needs
either `CAP_DAC_OVERRIDE` or a drop to uid 1000 — which is why the installer
uses `AT+SYSCMD` for that too.

## 6. Resource cost

Idle memory sat around 19 MB free of 164 MB total with `MemAvailable` ~97 MB.
Running a full dual-band scan did not measurably change `MemAvailable`
(97244 kB before, 97428 kB after). Load average during scanning stayed in the
range already set by the cellular daemon (~1.2). A scan takes ~4.5 s wall
clock.

The Wi-Fi driver and the cellular diag path are independent subsystems;
reloading `wlan.ko` repeatedly during the `con_mode` sweep did not disturb
`rayhunter-daemon`, which kept recording throughout (`1788231293-0.qmdl.gz`
grew across the whole session).

### Sustained scanning

Measured with `hardware-tests/scan_benchmark.sh`: seven minutes idle, seven
minutes scanning continuously on a dedicated interface, seven minutes idle
again. Cellular health is proxied by the growth rate of the active QMDL file —
a drop while scanning would mean diag messages were being missed.

| | Baseline | Scanning | Change |
|---|---|---|---|
| QMDL growth | 67.1 B/s | 66.8 B/s | **−0.4%** |
| `rayhunter-daemon` CPU | 7.88% | 7.86% | none |
| `MemAvailable` | 95604 kB | 95576 kB | ~0.5 MB held while the interface existed |
| `hostapd` | running | running | never dropped |

45 scans completed, 0 failed, one every ~9.5 s (a ~4.5 s scan plus a 5 s
pause). Load average went *down* over the scanning window (1.40 → 1.16),
which is measurement noise from the cellular side rather than a scanning
effect.

**The number that decides acceptability is the first row.** Cellular capture
was statistically unchanged under continuous scanning, so the feature does not
trade cellular reliability for wireless coverage. At this rate a recording
grows ~5.8 MB/day before radio evidence is added.

Still not measured, and needed before shipping a continuously-running daemon:
battery runtime impact, evidence-sidecar growth per day under a realistic
device population, and behaviour in a dense RF environment (this was a quiet
residential setting with 14–17 visible networks).

## 7. End-to-end verification

`rayhunter-radio-probe`, cross-compiled to `armv7-unknown-linux-musleabihf`
and run on the device via `AT+SYSCMD`:

```
=== hostapd before: 1
wrote 15 records to /tmp/radio_evidence.ndjson
scanned via rhscan0: 15 networks, 11 enabled signatures, 0 matches
probe rc=0
=== hostapd after: 1
	Interface wlan1  type AP
	Interface wlan0  type AP
```

The probe created `rhscan0`, scanned both bands, matched against the builtin
pack, wrote NDJSON evidence, and removed its interface. The hotspot stayed up
throughout and the cellular recording was unaffected. All 15 devices were
written pseudonymously because none matched a signature — no bystander's
address reached disk.

## 8. What this means for the phase plan

- **Phase 1 (BSS scanning, curated signatures, NDJSON evidence, UI):** viable
  now, demonstrated end to end.
- **Phase 2 (monitor mode, probe requests, IE fingerprints, Flock composites):**
  **blocked on the RC400L.** The rules can be written and tested, but nothing
  on this device can feed them. They ship disabled with that stated in their
  notes.
- **Phase 3 (persistence/following):** viable, but weaker than on a
  monitor-mode platform, because it can only track access points rather than
  the client devices that actually follow a person.
- **Phase 4 (BLE):** requires a companion radio. The observation API is built
  so one can be added without touching the analysis engine.
- **Phase 5 (Remote ID):** Wi-Fi Remote ID needs monitor mode, so it follows
  the same fate as Phase 2; BLE Remote ID follows Phase 4.

The honest framing for users: on this device Rayhunter can tell you which
*access points* are around you and flag ones belonging to surveillance
vendors. It cannot see the devices that merely listen or probe, which is where
much of the interesting behaviour lives.
