# KonnectONE Moxee Hotspot (K779HSDL)

Supported in Rayhunter since version 0.6.0.

The Moxee Hotspot is a device very similar to the Orbic RC400L. It seems to be
primarily for the US market.

**These devices have relatively little storage. The Orbic is usually a better alternative, though might be more expensive.**

- [KonnectONE product page](https://www.konnectone.com/specs-hotspot)
- [Moxee product page](https://www.moxee.com/hotspot)

## Supported bands

According to [FCC ID 2APQU-K779HSDL](https://fcc.report/FCC-ID/2APQU-K779HSDL), the device supports the following LTE bands:

| Band | Frequency               |
|------|-------------------------|
| 2    | 1900 MHz (PCS)          |
| 4    | 1700/2100 MHz (AWS-1)   |
| 5    | 850 MHz (CLR)           |
| 12   | 700 MHz (Lower SMH)     |
| 13   | 700 MHz (Upper SMH)     |
| 25   | 1900 MHz (Extended PCS) |
| 26   | 850 MHz (Extended)      |
| 41   | 2500 MHz (TDD)          |
| 66   | 1700/2100 MHz (E-AWS)   |
| 71   | 600 MHz                 |

## Installing

To get started, follow the [release installation guide](./installing-from-release.md). Then run the installer with the following command:

```bash
./installer moxee --admin-password 'mypassword'

# Note: the arguments --admin-username 'myusername' and --admin-ip 'mydeviceip'
#       may be required if different from the default.
```

* The password is the one used to log in to the device's admin menu. You can reset the password by pressing the button under the back case until the unit restarts.
   * ***Note:*** If you have changed the device username, password, or IP address from their default values, these must be provided as arguments to the installer command above.
* On Moxee-brand devices, check under the battery for the password.
* `./installer moxee` is almost the same as `./installer orbic`, it just comes with slightly better defaults that will give you more space for recordings. 

The Rayhunter UI will be available at <http://192.168.1.1:8080>.

<a name=shell></a>
## Obtaining a shell

After running the installer, there will not be a root shell and ADB will not be
enabled. Instead, you can use:

```bash
./installer util orbic-shell
```

## Keeping ADB across reboots

Enabling ADB on a Moxee normally lasts only until the device restarts: the boot
scripts reset the USB composition every time. To make it stick:

```bash
./installer util moxee-persist-adb --admin-password 'PASSWORD'
```

A Moxee and a home router very often both live at `192.168.1.1`, on different
interfaces, and which one your machine picks can change between runs. Sending a
device's admin password to the wrong one is worth avoiding, so there is a
wrapper that fingerprints the target first and refuses if it is not a Moxee
running Rayhunter:

```bash
./scripts/moxee-persist-adb.sh
```

It prompts for the password rather than taking it on the command line, so it
does not reach the process list or your shell history.

This writes `9` to `/usrdata/mode.cfg`, which is the persistent partition the
boot script reads to choose the USB composition. `9` gives RNDIS, DIAG, serial
and ADB; the factory default of `3` is RNDIS only. The value is read back after
writing, so the command fails rather than claiming success it cannot confirm.

Power cycle the device afterwards. A device booted on USB power alone disables
USB whatever this is set to, so power it off USB rather than just rebooting.

To undo it, add `--revert`.
