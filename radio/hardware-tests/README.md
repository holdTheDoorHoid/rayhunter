# Hardware test procedures (Orbic RC400L)

These are the scripts that produced the measurements in
[`../docs/capability-report-rc400l.md`](../docs/capability-report-rc400l.md).
They are kept so the findings can be re-checked on another unit — the RC400L
ships with more than one Wi-Fi chip depending on production run, so nothing
here should be assumed without re-running it.

## Running them

Every script needs the **full capability bounding set**. An adb shell does not
have it: `adbd` runs with only `CAP_SETUID|CAP_SETGID`, so `insmod`, `rmmod`
and even `ifconfig` fail with "Operation not permitted" regardless of uid.
Use the modem's AT command path, which is handled by an init-started daemon:

```bash
adb -s <serial> push radio/hardware-tests/phase0probe.sh /tmp/phase0probe.sh
adb -s <serial> shell 'chmod 755 /tmp/phase0probe.sh'
cargo run -p installer --bin installer -- util serial 'AT+SYSCMD=/tmp/phase0probe.sh'
```

`AT+SYSCMD` returns quickly, so anything long-running must be backgrounded by
a one-line launcher (`setsid /tmp/phase0probe.sh &`) and its output read from
the log afterwards.

## What each script does

| Script | Purpose | Log |
|---|---|---|
| `phase0probe.sh` | Full capability sweep: scan interface alongside the AP, scan timing, band coverage, monitor-vif attempt, `con_mode=12` reload, capture test | `/tmp/phase0_probe.log` |
| `conmode_sweep.sh` | Loads the driver at every `con_mode` from 0–14 and records the resulting interface type | `/tmp/conmode_sweep.log` |
| `monitor_alt.sh` | Alternative routes to frame capture: `iwpriv setMonChan`, `iw set type monitor`, pktlog, debugfs | `/tmp/monitor_alt.log` |

All three restore AP mode on every exit path, including on error, via a
`trap ... EXIT` handler.

## Safety notes

- **The scripts take the hotspot down briefly.** `phase0probe.sh` and
  `conmode_sweep.sh` unload the Wi-Fi driver. They restart `hostapd` and
  re-add `wlan0` to `bridge0` afterwards. USB adb is unaffected throughout,
  because it does not depend on Wi-Fi.
- **A reboot fixes anything these scripts break.** Module parameters are not
  persisted, so init brings the radio back up normally. The documented reboot
  for this device is `sync; echo b > /proc/sysrq-trigger` under `rootshell` —
  the `sync` is not optional.
- **Nothing here writes to a boot or firmware partition.** Enabling monitor
  mode or Bluetooth would require exactly that, and it is the one class of
  change that cannot be undone over USB.
- The cellular side is a separate subsystem. Reloading `wlan.ko` repeatedly
  did not disturb `rayhunter-daemon`, which kept recording throughout.

## Verifying the end-to-end path

After building `rayhunter-radio-probe` for the device:

```bash
cargo build -p rayhunter-radio --bin rayhunter-radio-probe \
  --target armv7-unknown-linux-musleabihf --profile firmware-devel
adb -s <serial> push \
  target/armv7-unknown-linux-musleabihf/firmware-devel/rayhunter-radio-probe /tmp/rhprobe
adb -s <serial> shell 'chmod 755 /tmp/rhprobe'
```

then run it via `AT+SYSCMD`. Expected output shape:

```
scanned via rhscan0: 15 networks, 11 enabled signatures, 0 matches
```

Check afterwards that `hostapd` is still running and that the evidence file
contains no raw MAC addresses for unmatched devices:

```bash
adb -s <serial> shell 'ps | grep -c hostapd; grep -c "id_type\":\"mac" /tmp/radio_evidence.ndjson'
```

Both counts matter: `1` for hostapd (the hotspot survived) and `0` for
retained addresses when nothing matched (the retention default held).
