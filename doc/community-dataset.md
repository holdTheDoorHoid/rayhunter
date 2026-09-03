# Contributing Recordings to a Community Dataset

Rayhunter can send recordings that raised a warning to a community-run
collection, so that patterns across many devices can be seen and captures can
be studied by people who know what to look for. This page is how to turn that
on, what to choose, and how to take a contribution back.

It is **off** until you turn it on. For what leaves the device and why the
collection side is built the way it is, read [How the Community Dataset
Protects You](./concepts/community-dataset.md) first. It is short, and the
choices below make more sense with it.

EFF does not run a collection service, and has said it will not. Anyone can run
one with the tools in this repository; the person who does is the one you are
trusting. Their service describes itself to your device, name and contact
included, before you send anything.

## What you need

- The address of a collection service, from whoever runs it. It starts with
  `https://`.
- If you want a map point for your recordings: a location on the device. Set
  that up under Settings → Recordings → GPS first. Without it, recordings are
  contributed without a location, which is fine.
- If you want uploads to happen on their own at home: the device joined to your
  WiFi as a client, under Settings → Network. Without it, choose "over WiFi or
  cellular data" below, or uploads never happen.

## Turning it on

1. Open the web interface and go to **Settings → Community**.
2. Tick **Contribute recordings to a community dataset**.
3. Enter the service address and press **Check server**. The device fetches the
   service's description and shows its name, what it accepts, and the
   fingerprints of its two keys. Nothing has been sent yet.
4. Compare the fingerprints with what the service's operator published, if they
   published them. Then press **Pin these keys**. From now on your device will
   only send to a service presenting exactly these keys.
5. Choose **what to send**. The shareable version is the default and is what
   this page assumes. See below before choosing the full recording.
6. Choose **which recordings**: any with a warning (the default), or only the
   more severe ones. Tick the baseline option only if the service's operator
   asked for clean recordings.
7. Choose a **location** precision. About 10 km is the default.
8. Choose **when to upload**. Only over WiFi is the default. If you name
   networks, uploads wait until the device is on one of them.
9. Press **Save**. The device restarts, which takes about a minute.

After the restart, the **Right now** box on the same page says what the device
is doing: waiting for WiFi, which recordings are queued, which it will not send
and why, and when the last upload succeeded.

## What the shareable version sends

The same thing as the **zip (shareable)** download on the history page, plus a
rounded location:

- The capture as a PCAP with this device's own IMSI, IMEI and temporary identity
  set to zero, and a report of how many were found and removed.
- The analysis report: which detector fired, when, on which packet.
- The device details with the home network and WiFi removed.
- The cells the device heard, by their network identities, without signal
  strengths.
- One location, rounded as you chose, or none.

Not sent, ever: the raw `.qmdl` capture, your recording names and notes, your
WiFi network's name, the recording currently being written, and any recording
containing a demo warning.

[Sharing What You Find](./sharing-findings.md) explains what the redaction does
and does not promise. The same limits apply here.

## Choosing the full recording

The full recording adds the raw capture, your identifiers intact, the home
network, signal strengths, and the location track at the precision you chose.
It is encrypted on your device to a key the service keeps **offline**, so the
internet-facing server cannot read it; only the person holding that key can.

You are trusting that person with who you are and where you were. The interface
says so in amber and asks you to tick that you understand. The device refuses to
send full recordings without that tick, and so does the service. Choose it when
a researcher you trust has asked for it, not by default.

## Kept out and taken back

- **Keep a recording out.** Open its row on the history page and press *Never
  contribute this one*. It stays on the device, and the community tab lists it
  under "not sending" with that reason.
- **Take a contribution back.** The same row shows *Contributed* with a date and
  a *Withdraw* button. Withdrawing sends the service a signed request to delete
  it, and the recording is never sent again. This works even months later,
  because the device keeps the key that signed it.
- **Stop linking your contributions.** The device signs with a key of its own
  that is replaced every 30 days, so the service can tell one device's
  contributions apart for a month and no longer. *New signing identity* on the
  community tab replaces it now.

## When uploads do not happen

The **Right now** box says why. The common reasons:

- *Waiting for the WiFi client to join a network.* The default policy is WiFi
  only. Join the device to a network, or choose the other policy.
- *Joined to a network that is not one of the allowed networks.* Add it, or
  clear the list.
- *Waiting for the minimum age.* Recordings wait an hour after they close by
  default, so that nothing is sent from where the warning happened.
- *The service presented keys other than the pinned ones.* Press Check server to
  see the new keys. If the operator announced a change, pin them; if not, do not.
- *Not enough free space.* Building the bundle needs room for a copy of the
  capture. Delete old recordings or add a memory card.

## Where to next

- [How the Community Dataset Protects You](./concepts/community-dataset.md), the
  reasoning behind each choice above.
- [Sharing What You Find](./sharing-findings.md), what the redaction does and does
  not promise.
- [Configuration Reference](./configuration-reference.md), every key under
  `[telemetry]`.
- To run a service yourself: `telemetry/collector/README.md` in the repository.
