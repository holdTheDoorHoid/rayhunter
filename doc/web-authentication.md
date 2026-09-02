# Securing the Web Interface

Only phones and computers that have been paired with your Rayhunter can use its
web interface, and everything between them travels encrypted. This page is how
pairing works day to day: the first time, adding another device, losing one,
and starting over. Why it is built this way is at the end.

## The first time

A Rayhunter that nobody owns yet shows a square code on its screen for ten
minutes after it starts. Pairing the first device is:

1. Join the Rayhunter's WiFi on your phone.
2. Scan the code with the phone's camera. If the phone cannot read it, open
   `https://rayhunter.local/pair` (or `https://192.168.1.1:8443/pair`) and type
   the eight characters printed under the code instead.
3. Your browser warns that the connection is not private. Choose to continue.
   The warning is expected, once per browser; the reason is under [Why the
   browser warns](#why-the-browser-warns).
4. Choose a passphrase of at least eight characters. Write it down somewhere
   you keep passwords. You will need it to add another device, and to open the
   terminal if that is enabled.

That phone is now trusted and is not asked again. The code leaves the screen
and does not come back.

*If the code has gone from the screen:* press any button on the Rayhunter. On
a unit nobody has paired with yet, a press shows the code for another ten
minutes. Once a device is paired, the button no longer does this.

*If the unit has no screen, or you cannot scan:* open the pairing page and
choose **Pair by pressing the button on the unit**. Press the button within
thirty seconds. The page then asks for the passphrase as above.

## Adding another phone or computer

From a device that is already paired, open **Configuration** and find **Who
can use this interface**. Choose **Add a phone or computer**. The page shows a
six-digit code, and the same code as a square to scan. On the new device, join
the Rayhunter's WiFi, then scan the square or open the pairing page and type
the code. The code is good for five minutes and one device.

Without a paired device to hand, open the pairing page on the new device and
enter the owner passphrase.

Either way the new device gets its own entry in the list, named from its
browser unless you name it yourself.

## The list of trusted devices

**Configuration → Who can use this interface** lists every paired device, when
it was added, and when it was last seen. The one you are using is marked. From
here you can:

- **Rename** a device, so "Android (Chrome)" becomes "Sam's phone".
- **Remove** a device. It stops working at once and would have to be paired
  again. Removing the device you are using sends you to the pairing page.
- **Change the owner passphrase.** The current one is required. Changing it
  does not remove any device.

## If you lose a device

Remove it from the list, from any other paired device. There is nothing on the
lost device that gives access to anything but this one Rayhunter, and only
until you remove it.

## If you lose every device and the passphrase

The pairing records are removed over USB, which needs the unit in your hand
and a computer with the installer:

```
./installer util reset-auth
```

The unit restarts into the first-time state, with the code on its screen.
Recordings and settings are untouched.

## The terminal

If the terminal was enabled when the unit was flashed, using it takes one
more step each time: the passphrase, and then a four-digit code that the
Rayhunter shows on its own screen (or a press of its button, on a unit with no
screen). This proves the person at the browser can also see the unit. The
terminal then stays open for five minutes, longer while it is being used,
and the unit's screen says **TERMINAL ACTIVE** the whole time.

## Why the browser warns

The Rayhunter signs its own certificate, and nobody on the internet vouches
for it, so the browser cannot tell it from an impostor and says so. Nothing
on the internet is involved: the unit is offline by design, and a certificate
that a public authority would vouch for cannot be issued to a private
address. You are asked to accept the warning once per browser, or you can
[stop it for good](#stopping-the-warning-for-good-optional).

If you want to check you are talking to your own unit, compare the
fingerprint the browser shows with the one on the pairing page or under
**Configuration**. They are the same forty hex digits.

## Stopping the warning for good (optional)

The unit has its own certificate authority, made on its first start. Tell a
phone or computer to trust that authority and the warning does not come back
on it, the padlock is shown, and `https://rayhunter.local` opens like any
other site. This is optional, and it is one device at a time; the unit never
asks for it.

Open the pairing page, or **Configuration → Who can use this interface**, and
expand **Stop the certificate warning for good**. The steps for your device
are there; in short:

- **iPhone or iPad:** download the profile, install it under **Settings →
  Profile Downloaded**, then turn on full trust under **Settings → General →
  About → Certificate Trust Settings**.
- **Android:** download the certificate and install it as a **CA certificate**
  from Settings. Firefox for Android needs its own setting turned on.
- **Mac:** download and install the profile, then mark the certificate
  **Always Trust** in Keychain Access.
- **Windows:** download the certificate and install it into **Trusted Root
  Certification Authorities**.
- **Linux, and Firefox anywhere:** import the certificate in the browser's
  own certificate settings, trusting it to identify websites.

The authority's fingerprint is shown beside the steps so you can check you
are installing your own unit's and not somebody else's. Trusting it trusts
this one unit and nothing more, and it is removed the same way it was
installed.

Behind it, the unit signs itself a short-lived server certificate, about
two years, and replaces it before it runs out. A device that trusts the
authority never notices the change.

## What this protects against, and what it does not

Everyone who knows a WiFi password can read everyone else's traffic on that
network. So before pairing, anyone given the Rayhunter's WiFi password could
read its recordings, and a password sent to the interface could be read by
anyone else on the WiFi. Now the connection is encrypted, and only paired
devices are answered. A guest, a family member, or anyone who was given the
WiFi for another reason gets the pairing page and nothing else.

It does not protect against someone holding the unit. The USB cable is root
on these devices, and that is by design: it is also how you recover. A unit
that nobody has paired with yet can be paired by whoever presses its button
during the ten-minute window, so pair a new unit before handing it to anyone.
And a phone that is paired is trusted until you remove it; a lost or stolen
phone should be removed from the list.

## Units updated from an earlier version

A unit that had web accounts keeps them for one purpose: signing in on the
pairing page pairs that browser, and the account's password becomes the owner
passphrase. Accounts no longer open the interface on their own, since they
crossed the WiFi unencrypted. A unit that had no accounts starts in the
first-time state after the update, with the code on its screen.

## Where to next

- [The Web Interface, Panel by Panel](./web-interface.md).
- [Configuration Reference](./configuration-reference.md), for the two
  settings involved, `tls_port` and `auth_store_path`.
- [Legal and Personal Risk](./concepts/risk.md), the wider picture of who can
  see what.
