# Securing the Web Interface

By default, anyone who can reach Rayhunter's web interface can use it, there is
no login. This fork adds optional accounts so you can put a password on it. This
page explains what that protection is, and, as importantly, what it is not.

## The honest headline

**There is no HTTPS on these devices.** That single fact shapes everything here.
Adding an account is a *second factor*, something beyond knowing the WiFi
password, not a secure channel. It stops someone who is already on the WiFi from
casually opening the interface. It does **not** encrypt the connection, and it
does not protect against someone able to watch the network traffic itself, who
could see the credentials go by. The interface says as much where you set this
up, and this page says it too so there is no confusion: accounts raise the bar,
they do not make the interface safe to expose.

## What the interface already does to protect itself

Accounts are one layer. This fork also hardened the interface itself, so some
protection is in place even before you add a password:

- **It is not served to the wider network.** The interface listens only on the
  device's own hotspot and on the device itself (loopback), not on the cellular
  or upstream (WAN) side. Someone out on the internet cannot reach it; reaching
  it still means being on the hotspot's WiFi.
- **It refuses cross-site requests.** A malicious web page you happen to open
  cannot make your browser quietly send state-changing commands to the device
  (starting or deleting recordings, changing settings). Such cross-site requests
  are rejected.
- **The login path and the optional terminal were hardened** against the usual
  ways each is abused.

None of this replaces the missing HTTPS, and none of it changes the advice
below. They narrow who can reach the interface and what a stray web page can do
through it; they do not make the connection private.

## Why it is off by default

Rayhunter has never required a login, and leaving it open by default is
deliberate: an update that suddenly demanded a password could lock someone out of
their own device, possibly one they are relying on. So authentication stays off
until you add an account. Adding one turns it on; removing all of them turns it
back off.

## When it is worth turning on

The interface serves everything Rayhunter knows, your recordings, your
warnings, your settings. On a hotspot, "anyone on the WiFi" can mean more people
than you intend. Consider adding an account when:

- Other people share the device's WiFi and you would rather they not read your
  recordings.
- You intend to turn on the display of this device's **own identity** (its IMSI,
  IMEI, and temporary identity). That data is off by default precisely because
  the interface may be open, and the IMSI is exactly what an IMSI catcher exists
  to collect. If you enable it, put a password on the interface first. See the
  Cell Site panel in [The Web Interface](./web-interface.md).

## How accounts behave

- **Adding or removing an account takes effect immediately.** An account added a
  moment ago already works; one removed already stops working.
- **Accounts are stored apart from the rest of the settings**, so a routine
  settings change cannot accidentally erase them (an earlier version of this
  feature had exactly that bug, now fixed).
- **Passwords are not shown back to you.** The interface redacts the stored
  credential rather than displaying it.

Manage accounts from the settings page. Because credentials cross an
unencrypted connection, choose a password you do not reuse anywhere important.

## What this does not replace

Putting a password on the interface is one layer. It does not change the fact
that:

- The device has no HTTPS, so the connection is not private on the network.
- The WiFi password itself remains a control worth keeping, anyone on the WiFi
  is already close to the interface.

If you genuinely need private access to the interface across an untrusted
network, that is beyond what these accounts provide, and beyond what these
devices currently support.

## A note on entering the password yourself

Setting or entering a web-interface password is something you do directly in the
interface. Rayhunter's documentation cannot do it for you, and you should be the
one to type it, treat it like any account credential.

## Where to next

- [The Web Interface, Panel by Panel](./web-interface.md), including the
  own-identity display that this protection matters for.
- [Configuration Reference](./configuration-reference.md), where the accounts
  setting sits among the rest.
- [Legal and Personal Risk](./concepts/risk.md), the wider picture of who can
  see what.
