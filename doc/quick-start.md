# Quick Start: From Box to First Recording

By the end of this page, you will have Rayhunter running on an Orbic RC400L and
watching for cell-site simulators. It takes about fifteen minutes, most of which
is downloading and waiting for the device to reboot.

This tutorial picks one device and one path and sticks to them, so that if you
follow it exactly, it works. If you have a different device, start at [Choosing a
Device](./supported-devices.md) instead; if this path fails on your setup, [the
from-source instructions](./installing-from-source.md) are the fallback. For
everyone with an Orbic, read straight down.

## What you need

- **An Orbic RC400L** hotspot, charged.
- **A computer** running macOS, Linux, or Windows.
- **The Orbic's WiFi password**, printed on the device or shown in its screen
  menu. On a Verizon Orbic this is also the admin password you will use below.

You do not need any programming knowledge, and you will not need to open the
device.

## Step 1: Download Rayhunter

Go to the [Rayhunter releases page](https://github.com/holdTheDoorHoid/rayhunter/releases)
and download the `.zip` for your computer's platform:

- macOS with Apple silicon (M1/M2/M3): `macos-arm`
- macOS with an Intel chip: `macos-intel`
- Linux on a normal PC: `linux-x64`
- Windows: `windows-x86_64`

**Expected result:** a file named something like
`rayhunter-vX.X.X-macos-arm.zip` in your Downloads folder.

*If you are not sure which Mac you have:* click the Apple menu → About This Mac.
"Apple M1" (or M2, M3) means `macos-arm`; "Intel" means `macos-intel`.

## Step 2: Unzip it and open a terminal there

Unzip the file, then open a terminal in the unzipped folder.

- **macOS/Linux:** unzip by double-clicking, then in a terminal run
  `cd ~/Downloads/rayhunter-vX.X.X-*` (replace with the real folder name).
- **Windows:** unzip in File Explorer, open the folder that contains
  `installer.exe`, hold **Shift**, right-click inside the folder, and choose
  "Open in PowerShell."

**Expected result:** a terminal whose current folder contains a file called
`installer` (or `installer.exe` on Windows). Run `ls` (macOS/Linux) or `dir`
(Windows) to confirm you see it.

*If you don't see `installer`:* you are in the wrong folder. Make sure you
unzipped the archive, and `cd` into the folder that was created, not the `.zip`.

## Step 3: Connect your computer to the Orbic's WiFi

Turn the Orbic on by holding its power button until the screen lights up. On
your computer, join the Orbic's WiFi network, using the password from "What you
need" above.

**Expected result:** open `http://192.168.1.1` in a browser. You should see the
Orbic's own admin page. That confirms your computer is talking to the device.

*If the page does not load:* make sure your computer is on the Orbic's WiFi and
not your home network or a wired connection. Give the Orbic a minute after
powering on to bring up its WiFi.

## Step 4 (macOS only): allow the installer to run

macOS blocks programs downloaded from the internet by default. Clear that flag
for the installer:

```bash
xattr -d com.apple.quarantine installer
```

**Expected result:** the command prints nothing and returns to the prompt.
(Linux and Windows users: skip this step.)

*If it says "No such xattr":* the flag was already absent. That is fine, continue.

## Step 5: Run the installer

Run the installer, giving it the Orbic's admin password. On a Verizon Orbic that
is the same as the WiFi password.

```bash
./installer orbic --admin-password 'YOUR_PASSWORD'
```

On Windows, run `.\installer.exe orbic --admin-password 'YOUR_PASSWORD'` in
PowerShell (not the old Command Prompt, which treats quotes differently).

Keep the quotes around the password. This step takes a couple of minutes, and
the terminal will print several lines as it works, that is normal.

**Expected result:** the installer prints progress and finishes with a message
telling you it is done and that the device will reboot.

*If it says "No Orbic device found" (macOS):* your security settings may be
blocking the connection. Open System Settings → Privacy & Security → check
"Allow accessories to connect," set it to "Always" temporarily, and run the
command again. Change it back afterward.

*If it cannot connect or the password is refused:* confirm the password by
reaching `http://192.168.1.1` in your browser and logging in there. On a Verizon
Orbic you can reset the password by holding the button under the back case until
the unit restarts.

## Step 6: Wait for the reboot

The Orbic restarts on its own. This takes up to a minute, during which the
screen may go dark or show the manufacturer's startup, that is expected, not a
failure.

**Expected result:** when it finishes starting up, a **green line flashes along
the top of the Orbic's screen.** That green line is Rayhunter running and
recording. You are almost done.

*If no green line appears after two minutes:* power the device fully off and on
again. If it still does not appear, the [troubleshooting
page](./troubleshooting.md) covers what to check.

## Step 7: Open Rayhunter's dashboard

With your computer still on the Orbic's WiFi, open:

```
http://192.168.1.1:8080
```

Your browser will warn that the connection is not secure. That is expected, Rayhunter does not use HTTPS on the device, and you can safely continue past
the warning.

**Expected result:** Rayhunter's web dashboard, showing a recording already in
progress and panels for the current status. This is the home base for everything
else.

*If the warning has no "continue anyway" option:* different browsers hide it
differently, look for "Advanced," "Show details," or "visit this website."
*If the page does not load at all:* re-check that you are on the Orbic's WiFi and
that you included `:8080` at the end of the address.

## You are done

Rayhunter is installed, running, and recording. The green line on the device and
the dashboard in your browser are the two signs of success. From here it keeps
watching on its own whenever the Orbic is powered on.

Two things worth doing next, in order:

1. **Learn what a warning looks like before you ever get a real one.** [Your
   First Warning](./first-warning.md) walks you through producing a harmless
   practice warning and reading it, so the real thing does not catch you cold.
2. **Understand what the tool can and cannot tell you.** [Reading Warnings
   Without Panicking](./concepts/interpreting-warnings.md) is the most important
   page in this book once warnings start appearing.

If you want to know what you now have running and how it works, the [Understanding
the problem](./concepts/cell-networks.md) section explains it from the ground up.
