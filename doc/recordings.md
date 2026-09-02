# Recordings: Naming, Notes, and Rotation

A recording is one capture session, the signalling your device saw between the
moment recording started and the moment it stopped. This page covers the tools
this fork adds for managing them: naming them, adding notes, splitting them
automatically, and clearing out the ones that found nothing. All of these are
fork additions.

For opening a recording and reading what it found, see [The Web Interface, Panel
by Panel](./web-interface.md) and [The Packet Explorer](./packet-explorer.md).

## Names and notes

By default a recording is identified only by its number and time. You can give it
a **display name** and free-text **notes** from the recording history. This does
two useful things:

- It makes a recording findable later, "Federal Plaza, Tuesday" beats a
  timestamp when you are looking back through a long list.
- The display name is used for the **downloaded file**, so an exported recording
  arrives with a meaningful name instead of an opaque identifier.

The display name is limited to a short set of plain characters (letters, digits,
and underscores, up to 29). That limit is deliberate and protective: the name
ends up in a filename and in a download header, so restricting it prevents a name
from doing anything unexpected there. Notes are free text and have no such limit.

Naming a recording also protects it from automatic deletion, described below.

## Rotation: splitting a recording automatically

A single recording left running for a long time becomes one very large file,
which is slow to download over the device's own WiFi and cannot be analysed until
it is closed. Rotation avoids that by starting a fresh recording automatically:

- **By size**, set `max_recording_size_mb` to close the current recording and
  start a new one once it reaches that size.
- **By time**, set `max_recording_minutes` to do the same after a length of
  time.

If you set both, whichever limit is reached first triggers the rotation. Rotation
is off by default, because silently splitting a capture into pieces would surprise
someone who did not ask for it. Splitting has a second benefit beyond file size:
each closed piece is analysed and readable while capture continues, instead of
everything waiting until you stop.

## Auto-deleting clean recordings

On a device that records for long stretches, storage fills up. With
`auto_delete_clean_recordings` on, Rayhunter reclaims space by deleting
recordings that found nothing, but only under strict conditions, and this is
worth understanding precisely because it deletes your data:

A recording is eligible for automatic deletion only if **all** of these are true:

- It has been analysed and its report **cleanly shows no warning**. A report
  that is empty, truncated, or corrupt does not count as clean: it is treated
  as unknown, and the recording is kept. Deletion fails closed, so a damaged
  report can never be mistaken for "found nothing."
- It is **not named**. A display name marks a recording as one you care about,
  and a named recording is never auto-deleted.
- It is **not the recording currently being written**.
- It is **not still waiting** to be uploaded (if WebDAV upload is configured).

When space runs low, eligible recordings are removed oldest first. Anything not
understood, anything that found something, and anything you named is kept.
Auto-deletion is off by default, because deleting someone's captures without
being asked is not something to do quietly, however good the reason.

The practical rule: **if a recording matters, name it.** A name both labels it
and shields it from cleanup.

## Where recordings are stored

Recordings go to the device's internal storage unless you choose a memory
card on the settings page, under **Storage Management**. The page lists the
internal storage and every card it can see, with the free space on each, and
takes a path of your own for anything it cannot see. The TP-Link is the
device this is for: its own flash holds an hour or two of recording, a card
holds weeks.

A card can be pulled at any moment, or not be there when the device starts.
Rayhunter checks it every few seconds. While the card is missing, or is
there but cannot be written, recordings go to internal storage; when it
returns, they move back. Each change closes the recording in progress, noting
the reason on it, opens a fresh one in the new place, and sends a **Storage**
notification if you have notifications set up. The System Information panel
says where recordings are going right now, and why, if it is not the card.

Two things follow from this that are worth knowing. The recordings list shows
what is in the place recordings are currently going: while the card is out,
the recordings on it are not listed, and the ones made meanwhile stay on
internal storage when the card comes back. And Rayhunter mounts the card
itself if the system has not, so a card inserted after boot is picked up
without a restart.

## Downloading and sharing

Recordings download as a bundle containing the raw capture, its analysis, and
the device details saved when the recording started: what hardware and
Rayhunter build made it, the home network it was analysed against, the
device's clock and its correction, and how much room the device had. The
same details are shown under **Device details** on each recording's card.
What that bundle contains, including identifiers that should be removed before
you show it to anyone, is the subject of [Sharing What You
Find](./sharing-findings.md). Read that before sending a recording anywhere.

Recordings made before this version have no device details; their card says
so.

## Where to next

- [Configuration Reference](./configuration-reference.md), the exact keys and
  defaults for rotation and auto-deletion.
- [Sharing What You Find](./sharing-findings.md), exporting and redacting a
  recording.
- [Re-analyzing Recordings](./reanalyzing.md), running the detectors over a
  recording again.
