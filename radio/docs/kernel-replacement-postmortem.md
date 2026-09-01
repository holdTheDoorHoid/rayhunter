# Why the replacement kernel bricked the device

Unit `49514baf` was flashed with a kernel built from the public CAF
`msm-3.18` tree at `caf_migration/LE.UM.1.3.r1.8` (3.18.48, matching the
device's own `uname`). It boot-loops: the screen flashes the welcome logo
every few seconds and the device never enumerates on USB at all — no adb, no
fastboot. Neither button combination reached the bootloader, so it is not
currently recoverable.

**Do not repeat this on another unit.** The cause is now known and it is not
something a config tweak fixes.

## The cause

Diffing the device's own `/proc/config.gz` against the config my build
actually resolved to shows **21 symbols enabled on the device that do not
exist anywhere in the public CAF tree**:

```
CONFIG_ARM_ARCH_TIMER_VCT_ACCESS   CONFIG_KEY_POWEROFF
CONFIG_CHARGE_ETA6003              CONFIG_KEY_WPS_RESET
CONFIG_FB_DEFERRED_IO              CONFIG_MSM_IPC_ROUTER_USB_XPRT
CONFIG_FB_SYS_COPYAREA             CONFIG_USB_F_IPC
CONFIG_FB_SYS_FILLRECT             CONFIG_WAKEUP_OUT
CONFIG_FB_SYS_FOPS                 CONFIG_WAKEUP_REPORT
CONFIG_FB_SYS_IMAGEBLIT            CONFIG_WIFI_POWER
CONFIG_FB_TFT                      CONFIG_HW_ID
CONFIG_FB_TFT_FBTFT_DEVICE         CONFIG_I2C_AW9523
CONFIG_FB_TFT_ST7735S              CONFIG_INPUT_SX9310
CONFIG_IPC_ROUTER_NODE_ID=2
```

Every one of them returns "not in this tree" when grepped for as a Kconfig
symbol. Not even `drivers/staging/fbtft` is present. These are Orbic/Wingtech
additions to a **private vendor kernel tree**, and `make olddefconfig`
silently dropped all of them because nothing defines them. That is also why
my zImage came out 87 KB smaller than the vendor's.

**The public CAF tree is not the source this device's kernel was built from.**

## Prime suspect for the loop

`CONFIG_ARM_ARCH_TIMER_VCT_ACCESS` — a vendor patch to the ARM architected
timer. A kernel whose timer setup does not match what the firmware expects
hangs before the console or USB gadget comes up, which is exactly the observed
symptom: no output, no enumeration, watchdog reset, repeat.

## The deeper point: even a booting kernel would have been useless

This matters more than the boot failure, because it means "fix the hang and
retry" was never going to work:

| missing | consequence |
|---|---|
| `CONFIG_FB_TFT_ST7735S`, `FB_TFT`, `FB_SYS_*` | **no display** — Rayhunter's whole screen feature reads `/dev/fb0` |
| `CONFIG_KEY_POWEROFF`, `CONFIG_KEY_WPS_RESET` | **no buttons** — including the fastboot key combination that is the only recovery path |
| `CONFIG_WIFI_POWER` | **possibly no Wi-Fi at all** — a vendor driver that almost certainly drives the WLAN power rail or enable GPIO |
| `CONFIG_CHARGE_ETA6003` | no battery charger |
| `CONFIG_HW_ID`, `I2C_AW9523`, `INPUT_SX9310` | no hardware ID, GPIO expander, proximity sensor |

The `CONFIG_WIFI_POWER` line is the one that retires the whole approach. The
entire point of replacing the kernel was to reach monitor mode via
`ath10k_sdio`. If the vendor driver that powers the WLAN chip is absent, a
custom kernel may not be able to turn the radio on at all — so the best case
was a device with no screen, no buttons, no charging, and quite possibly no
Wi-Fi either.

## What was actually sound

Worth separating, because none of this was the failure:

- The flash mechanism worked: erase, write, and read-back verify all passed,
  md5 identical.
- `mkbootimg_rc400l.py` is correct — it round-trips the original image byte
  for byte, SHA1 id included.
- `fuse_flag=0` was read correctly; the bootloader did accept an unsigned
  image and jumped to it. Secure boot was never the obstacle.
- Backups of boot, recovery, aboot and misc are intact on the host.

The failure was entirely "the kernel I built is not this device's kernel".

## What would be needed to try again

The **vendor kernel source**. It is a GPL-licensed work being distributed in a
shipped product, so Orbic (or Wingtech, who appear to be the ODM — the vendor
build path in the shipped driver is
`.../mdm9x07/ap/le2.0.1_ap_vzw/...`) are obliged to provide it on request.
Without it, no kernel built for this device can carry its display, buttons,
charger or Wi-Fi power control.

Absent that source, the honest position is that **monitor mode is not
reachable on the RC400L**:

- the vendor Wi-Fi firmware refuses a monitor vdev (proven directly)
- both firmware builds on the device behave identically
- RX-filter and promiscuous WMI parameters are ignored
- `ath10k`, which does support monitor mode on this exact chip, needs a
  wireless stack this kernel cannot host, and the kernel cannot be replaced
  without vendor source

## Recovery status of unit 49514baf

Not currently recoverable by software. Remaining options, in order of
plausibility:

1. **EDL / 9008 mode.** Qualcomm's emergency download, usually reached by
   shorting test points on the PCB. Requires a signed firehose programmer for
   MDM9207, which we do not have — these are sometimes obtainable from vendor
   flash-tool packages.
2. **A different key combination or timing** than the two tried.
3. **Serial console.** The board exposes `ttyHSL0` at 115200 on the debug UART
   (`0x78b3000`); if those pads are reachable it would at least show where the
   kernel dies, and aboot may offer a prompt.
