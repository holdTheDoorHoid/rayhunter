# Netgear Nighthawk M6 / M6 Pro (MR6xxx)

> **Status: work in progress — scaffold merged, not yet verified on hardware.**
> The installer knows this device and the install-over-telnet path is the same
> proven pipeline used for the TP-Link and Wingtech. The one piece not yet
> verified is getting the first root shell (see [Getting root](#getting-root)).
> Track it in the issue linked from the pull request that added this page.

## The device

The Nighthawk M6 (MR6110/MR6150/MR6450) and M6 Pro (MR6500 retail, MR6550 AT&T)
are Qualcomm-based 5G hotspots with a 2.8" colour touchscreen.

| | |
| --- | --- |
| SoC (M6 Pro) | Snapdragon X65 — `sdxlemur`, SDX65, ARMv7 Cortex-A7 |
| SoC (M6) | Snapdragon X55 — `sdxprairie`, SDX55, ARMv7 Cortex-A7 |
| RAM | ~650 MB (far roomier than the LTE hotspots) |
| Userspace | QTI Linux with Qualcomm's QCMAP mobile-AP / web stack |
| USB (RNDIS tether) | `0846:68e1`, product string `MR6X00` |
| Admin web UI | `http://192.168.1.1/`, session-gated |

Two things make this an attractive target:

- **The daemon binary already exists.** Rayhunter ships an
  `armv7-unknown-linux-musleabihf` build, statically linked against musl. The
  SDX55/SDX65 are ARMv7, so that same binary is the right one — no new build
  target is needed.
- **The modem's DIAG is on-SoC**, so `/dev/diag` (the daemon default) is
  expected to be correct. If a given firmware exposes it elsewhere, set
  `diag_device_path` in `config.toml`.

## Getting root

This is the open item. Rayhunter needs a root shell to install; on the MR6xxx
that means opening telnet on TCP 23. The documented community routes:

1. **Older firmware (≈ MR6xxx 10.x):** an AT interface listens on TCP 5510 and
   a root telnet can be opened on TCP 23 with no exploit. If your unit already
   answers telnet on 23, the installer detects it and proceeds.
2. **B.Kerler's `mrCONFIG` keygen** derives a per-device telnet-unlock token
   and unlocks telnet across the MR6xxx line. Run it, then run the installer.
3. **AT&T 12.x+** locks the above down. The community workaround is
   cross-flashing MR6550-100PAS firmware onto an MR6500-1A1NAS to regain root
   telnet. **This is a brick risk and is deliberately not automated by the
   installer.**

The MR6xxx web UI is Qualcomm QCMAP — the same CGI backend
(`/cgi-bin/qcmap_auth`, `/cgi-bin/qcmap_web_cgi`) the Wingtech installer already
exploits. Finishing the in-installer web enable means capturing the M6's login
exchange (its token/challenge scheme, not the Wingtech AES-ECB key) and the
injectable field, then wiring them into `try_enable_telnet()` in
[`installer/src/netgear.rs`](../installer/src/netgear.rs). Until that is
verified on a unit, the installer does **not** pretend to root the device: it
enables telnet only if you have opened it another way.

## Installing

Once root telnet is open on port 23 (by any route above), connect to the
hotspot over Wi‑Fi or USB tethering and run:

```sh
./installer netgear --admin-ip 192.168.1.1
```

The installer detects the open telnet, pushes the daemon to
`/data/rayhunter`, installs the init script, and reboots. Afterwards the web
interface is at `http://192.168.1.1:8080`.

Once the web-based enable is finished and verified, you will instead be able to
pass the admin password and let the installer open telnet itself:

```sh
./installer netgear --admin-password 'YOURPASSWORD'
```

## Obtaining a shell

For bringing the port up on real hardware:

```sh
# Uses an already-open telnet, or (once implemented) opens it with the password
./installer util netgear-shell --admin-ip 192.168.1.1
```

```sh
./installer util netgear-start-telnet --admin-ip 192.168.1.1
telnet 192.168.1.1
```

## Hardware bring-up checklist

For whoever tests this on a unit, confirm and record:

- [ ] Root telnet reachable on `192.168.1.1:23` (by which route).
- [ ] `/dev/diag` exists and is readable (`ls -l /dev/diag`). If not, find the
      DIAG node and set `diag_device_path`.
- [ ] A writable, persistent partition for `--data-dir` (default
      `/data/rayhunter-data`) and how much free space it has (`df -h /data`).
- [ ] `update-rc.d` is present (the init script falls back to rc links if not).
- [ ] After reboot: the daemon runs and the web UI answers on `:8080`.
- [ ] Chipset/firmware strings: `cat /proc/device-tree/model`,
      `cat /sys/devices/soc0/machine`, `cat /etc/version`.

Please share those findings on the tracking issue so this page can be
completed and the device promoted from "in progress" to "functional".

## References

- [Porting to a New Device](./porting.md)
- B.Kerler's `mrCONFIG` / MR6xxx tooling (telnet unlock keygen).
