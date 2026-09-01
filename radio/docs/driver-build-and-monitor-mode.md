# Building the Orbic's Wi-Fi driver, and why monitor mode still fails

Follow-on to [`capability-report-rc400l.md`](./capability-report-rc400l.md),
which concluded that monitor mode was unreachable. That conclusion was right
in outcome but wrong in reasoning, and the reasoning matters — it changes what
could still be tried.

## Summary

| | Result |
|---|---|
| Build `wlan.ko` from matching sources | **Works.** Loads and runs identically to the vendor module |
| `con_mode=4` gives a radiotap netdev | **Yes.** `ARPHRD_IEEE80211_RADIOTAP` (803) |
| Host-side monitor setup | **All of it succeeds** |
| Firmware answers `VDEV_START` for a monitor vdev | **No. Silently ignored** |
| Firmware answers `VDEV_START` for an AP vdev | **Yes, `status=0`** |

**The blocker is the closed-source firmware blob, not the driver.** No amount
of host-side work fixes it.

## Correcting the earlier report

Two claims in the Phase 0 report were wrong:

1. **"`VOS_MONITOR_MODE` is 12."** It is **4**. The enum in
   `CORE/VOSS/inc/vos_types.h` places `VOS_MONITOR_MODE` *before* the
   explicitly-numbered `VOS_FTM_MODE = 5`.
2. **"No `con_mode` value exposes monitor mode."** `con_mode=4` does. The
   original sweep only looked at `iw dev`, which reports the cfg80211 iftype
   and says `managed` regardless. The netdev's `ARPHRD` type is the correct
   measurement, and it reads 803 — radiotap — exactly as monitor mode should.

The right measurement changed the answer. `iw` was never going to show this,
because in this driver monitor mode is not a cfg80211 interface type at all:
it is a driver-global mode selected at load time, with its own netdev ops
(`wlan_mon_drv_ops`) and its own channel-setting ioctl (`iwpriv setMonChan`),
none of which go through cfg80211.

## The build

Matching the shipped artefacts exactly:

| Component | Source | Version |
|---|---|---|
| Kernel | `clo/la/kernel/msm-3.18` @ `caf_migration/LE.UM.1.3.r1.8` | 3.18.48 |
| Driver | `clo/la/platform/vendor/qcom-opensource/wlan/qcacld-2.0` @ `caf_migration/LE.UM.1.3.r1.1` | 4.0.11.205G |
| Toolchain | Bootlin `armv7-eabihf--glibc--stable-2018.02-1` | gcc 6.4.0 |

The driver version was found by scanning every branch's
`CORE/MAC/inc/qwlan_version.h` for the `4.0.11.205G` the device reports; the
kernel by scanning branches for `SUBLEVEL = 48`. Note they are **different
release numbers** (r1.1 and r1.8) — the vendor did not build both from one
tag, so matching on version strings rather than on branch names matters.

Three things made this feasible, all verified in `/proc/config.gz`:

- `# CONFIG_MODVERSIONS is not set` — no symbol CRCs, so no need for the
  original `Module.symvers`.
- `# CONFIG_MODULE_SIG is not set` — no signature required.
- Therefore the only load-time gate is the vermagic string, and
  `touch .scmversion` in the kernel tree keeps `UTS_RELEASE` at a bare
  `3.18.48` so it matches byte for byte.

Host-side workarounds, both in [`build-wlan.sh`](../hardware-tests/build-wlan.sh):

- The CAF build routes every compile through `scripts/gcc-wrapper.py`, which
  is Python 2. Bypassed with `CC=arm-linux-gcc`.
- The 2018-vintage toolchain wants `libmpfr.so.4`, which current Ubuntu does
  not ship. Shimmed by extracting an old `libmpfr4` package into a local
  directory and pointing `LD_LIBRARY_PATH` at it.
- `scripts/dtc` fails to link under modern gcc's `-fno-common` default;
  `HOSTCFLAGS="-fcommon -w"` fixes it.

One trap worth recording, because it produces a module that builds cleanly and
then refuses to load: **do not pass `KBUILD_CFLAGS_MODULE`** to add warning
suppressions. It *overrides* the kernel's own value, which includes `-DMODULE`,
and without that every `MODULE_INFO()` compiles away — the resulting `wlan.ko`
has no vermagic and no description. Use `KCFLAGS`, which appends.

### Validation

An unmodified rebuild was loaded on the device before anything was changed:

```
insmod rc=0
wlan: loading driver v4.0.11.205G
Host SW:4.0.11.205G, FW:0.0.0.9, HW:QCA93x7_REV1_1
target uses HTT version 3.28; host uses 3.28
hostapd running: 1
scan rc=0 networks=19
```

Identical banner to the vendor module, hotspot up, scanning works. The
pipeline is trustworthy.

## What instrumentation showed

`pr_err()` probes were added at each step of the monitor path (the patch is
[`qcacld-monitor-instrumentation.patch`](../hardware-tests/qcacld-monitor-instrumentation.patch)).
Loading with `con_mode=4`:

```
RHMON: vdev_attach id=0 type=4 subtype=0     <- WMI_VDEV_TYPE_MONITOR
RHMON: self_peer set for monitor
RHMON: __hdd_mon_open entered
RHMON: WLANTL_RegisterSTAClient -> 0         <- success
RHMON: sme_create_mon_session -> 0           <- success
RHMON: wma_set_channel vdev=0 chan=6 vdev_up=0 conparam=4
RHMON: calling wma_vdev_start vdev=0 chan=6
```

Every host-side step succeeds. The driver creates the vdev as
`WMI_VDEV_TYPE_MONITOR`, sets raw decap mode, creates the monitor self-peer,
registers the RX callback, and issues `VDEV_START` on channel 6.

And then nothing. No `VDEV_START_RESP`, and no rx indication ever reaches
`htt_t2h` — the firmware sends not one frame.

The control settles it. The same instrumented driver in normal AP mode:

```
RHMON: VDEV_START_RESP vdev=0 status=0
```

**The firmware answers `VDEV_START` for an AP vdev and silently drops it for a
monitor vdev.** `qwlan30.bin` (FW `0.0.0.9`) does not implement
`WMI_VDEV_TYPE_MONITOR`.

## What is left to try

Ranked by expected value, and all still open:

1. **A different firmware blob.** `/lib/firmware/qwlan30.bin` is an ordinary
   file, not a boot partition, so swapping it is fully reversible over USB —
   the lowest-risk experiment remaining. Some ROME/QCA9377 firmware builds do
   implement monitor mode. Needs a build compatible with HTT 3.28 and this
   board data.
2. **Promiscuous management-frame capture without a monitor vdev.** The WMI
   headers define `WMI_PDEV_PARAM_RX_FILTER` and
   `WMI_PDEV_PARAM_SET_PROMISC_MODE_CMDID`, and the firmware already forwards
   management frames to the host via `WMI_MGMT_RX_EVENTID` during scans. If
   the filter can be widened on a normal vdev, that yields probe requests —
   which is the actual requirement — on firmware that demonstrably works.
   Whether this firmware implements those parameters is untested.
3. **pktlog.** `/proc/ath_pktlog/cld` and `/proc/sys/ath_pktlog/cld/` exist and
   the driver exports `ath_sysctl_pktlog_enable`. Carries RX descriptors;
   whether it yields foreign management frames is unknown.

Avenue 2 is the most interesting, because it does not depend on finding a
firmware blob that may not exist, and because management frames are precisely
what surveillance detection needs.

## What the firmware already gives us

Avenue 2 was measured. `tlshim_mgmt_rx_process` is the single point where
every management frame from firmware enters the host, so it was instrumented
with a per-subtype census and the device left running as an ordinary access
point for three minutes:

```
RHMON: mgmt census total=200 assocreq=0 probereq=0 probersp=19 beacon=181 auth=0 deauth=0 action=0
```

Two things follow, and they pull in opposite directions.

**The good half:** 181 of 200 frames were **beacons from other access points**.
The firmware is already doing promiscuous management-frame reception and
handing foreign frames to the host, in ordinary AP mode, with no monitor vdev
and no special configuration. That path is live and free.

**The bad half:** **zero probe requests**, across three minutes in a
residential area where phones probe constantly. The firmware is filtering them
out before the host ever sees them — most likely probe-response offload, where
the firmware answers probes itself and never troubles the host.

So the frames we can currently get (beacons, probe responses) are the same
frames BSS scanning already provides. The frames we actually need for current
Flock detection — probe requests — are withheld by the firmware, exactly as
monitor mode is.

The remaining question for this avenue is whether the RX filter can be widened
to include probe requests. A first attempt to read the firmware's WMI service
bitmap suggested `WMI_SERVICE_VDEV_RX_FILTER` is not advertised:

```
RHMON: fw services bitmap 0000000d 00000007 00000005 00000003 0000000f 0000000d 0000000b 0000000f
RHMON: VDEV_RX_FILTER=0 MGMT_TX_WMI=0 MGMT_TX_HTT=0 PACKET_FILTER=0 BPF=0
```

**Treat that reading as unverified.** The bitmap is implausibly sparse for a
firmware that demonstrably supports AP mode, offloaded scanning and beacon
reception, so the decode is more likely wrong than the firmware is that
limited. Even if the service bit really is absent, the parameters are worth
issuing directly — service bits are advisory and firmware often implements
more than it advertises.

## Both remaining avenues were tested, and both failed

### Swapping the firmware blob

The device carries **two different ROME firmware builds**:

| | size | date | md5 |
|---|---|---|---|
| `/lib/firmware/qwlan30.bin` (in use) | 619364 | Aug 2020 | `8c03e4ba…` |
| `/firmware/image/qwlan30.bin` (unused) | 613948 | Apr 2020 | `e7040b3f…` |

The `otp30.bin` and `bdwlan30.bin` alongside them differ too. Swapping in the
alternative build is fully reversible — these are ordinary files, not a boot
partition.

The alternative firmware **works**: it boots, reports `Target Ready`, the same
`HTT version 3.28`, the same `Host SW:4.0.11.205G, FW:0.0.0.9,
HW:QCA93x7_REV1_1`, and scanned 21 networks. So it is a valid, functioning
image, not a dud.

And it refuses monitor mode **identically**: `VDEV_START_RESP` count 0,
`rx_packets` 0 -> 0. Both firmware builds on this device reject a monitor
vdev the same way.

### RX filter and promiscuous mode

A writable module parameter was added so arbitrary WMI parameters could be
issued live without rebuilding per experiment. Three were tried against a
running AP, each with a 45-second observation window:

| parameter | value | probe requests seen |
|---|---|---|
| `WMI_PDEV_PARAM_SET_PROMISC_MODE_CMDID` (96) | 1 | 0 |
| `WMI_PDEV_PARAM_RX_FILTER` (85) | `0xFFFFFFFF` | 0 |
| `WMI_PDEV_PARAM_RX_FILTER` (85) | `0xA0` | 0 |

All three were accepted by the host path (`ret=0`), which only means the WMI
command was queued. The firmware ignored all of them: not one probe request
was delivered in any stage.

### A method that did not work, recorded so it is not repeated

The firmware blobs were searched for `monitor`, `promisc` and `sniff` symbol
strings, all of which returned zero, and that was briefly taken as
confirmation. It is not evidence: the same search returns zero for `beacon`,
which the firmware certainly implements. These images carry only a small
fragment of a symbol table, so absence proves nothing. The empirical
`VDEV_START_RESP` test is the only sound evidence here.

## Where this leaves monitor mode

Every avenue reachable without replacing the wireless stack has now been
tried and failed:

- monitor vdev — firmware refuses
- both on-device firmware builds — refuse identically
- RX filter and promiscuous parameters — firmware ignores
- mainline `ath10k`, which does support monitor mode and explicitly claims
  this chip's SDIO id (`0271:0701`) — blocked because this kernel has
  `CONFIG_MAC80211` unset entirely and `CONFIG_CFG80211=y` built in, so
  neither a module build nor the `backports` project can substitute a newer
  stack

What remains is a kernel replacement: build a kernel with `CFG80211=m` and
`MAC80211=m`, backport `ath10k_sdio`, flash it, and use the ath10k QCA9377
SDIO firmware. That means writing a boot partition — the one class of change
that cannot be undone over USB — and the payoff is not assured, since ROME
firmware's monitor support under ath10k is itself unproven.

The pragmatic alternative is the one every reference project in this space
already took: flock-you, Sky-Spy and OUI-SPY all run on an ESP32, because
ESP32 promiscuous mode is trivial and reliable. The observation API in
`rayhunter-radio` was built for exactly that — an external source feeds the
same analysis engine, and nothing downstream changes.

## Reproducing

```bash
./radio/hardware-tests/build-wlan.sh -j8
arm-linux-strip --strip-debug qcacld-2.0/wlan.ko
adb -s <serial> push qcacld-2.0/wlan.ko /tmp/wlan_new.ko
# needs CAP_SYS_MODULE, so run it through the modem's AT command path
installer util serial 'AT+SYSCMD=/tmp/runtestdrv.sh'
```

`test_custom_driver.sh` loads from `/tmp` and never replaces the vendor module
on disk, so a reboot always restores the stock driver. Throughout this work the
device stayed reachable over USB and the cellular daemon kept recording; the
only incident was a USB re-enumeration after many rapid driver reloads, which
recovered on its own without a reboot.
