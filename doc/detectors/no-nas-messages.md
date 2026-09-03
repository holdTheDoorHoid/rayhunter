# No Conversation With the Network (experimental)

Warns, once per recording, when five minutes of modem traffic go by without
the SIM and the network exchanging a single message. That is what a dead,
unactivated or badly seated SIM looks like from inside a recording.

This detector comes from **upstream** (EFForg/rayhunter#1132) and is **off by
default** there and here. It is marked experimental by its authors.

## What you would see

One Low warning in the recording, reading "No NAS messages seen in 5 minutes,
SIM possibly not working", and then nothing more from this detector for the
rest of that recording. It says nothing about any tower. It says the recording
is unlikely to catch anything, because the thing a fake tower has to do, talk
to the SIM, is not happening.

The live equivalent is the SIM verdict in the
[cell-site panel](../web-interface.md), which reads "SIM may not be working"
in the same situation. This detector leaves the same finding in the saved
recording, where it survives export and re-analysis.

## Why it matters

Every detector Rayhunter has, apart from the broadcast checks, works on the
conversation between the SIM and the network core: identity requests,
authentication, encryption choices, rejections. A SIM the network never
accepts produces none of it. The device keeps running, the screen stays
green, and the quiet looks like a quiet night rather than a recording that
was never going to find anything. Upstream added this after finding that
"an unactivated SIM might work" was not an answer people could act on.

## When it fires harmlessly

Often enough that it is off by default.

- **No network to reach.** A device that can hear towers but belongs to no
  network they serve, a foreign SIM with no roaming, say, or a device whose
  radio bands do not match the local networks, never talks to a core and is
  flagged every recording.
- **Turned away, then quiet.** A network that rejects the SIM once is not
  tried again for a long while. A recording started after that rejection
  sees towers and no conversation, and is flagged even though the SIM is
  fine and the earlier recording would show the rejection.
- **A device attached before recording started.** Not a false positive in
  practice, because an attached device still exchanges tracking area updates
  and paging within minutes, but a device that is genuinely idle for the
  whole five minutes would be.

## How it works

Every message the modem logs carries a timestamp. The detector remembers the
first one it sees and watches the clock advance, message by message, whether
or not Rayhunter could decode the message. Once five minutes of recorded time
have passed without a single NAS message, the layer that carries the
conversation between the SIM and the core, it raises one Low warning and
stops. The moment a NAS message appears, it stops without warning.

The clock is the recording's own, not the device's wall clock, so
re-analysing a recording gives the same answer. A jump backwards, or a single
jump of five minutes or more, is treated as a clock correction rather than
five minutes of silence, and the window restarts.

## Precise behavior

- Timestamps advance on every logged message, decoded or not.
- One **Low** warning when the span from the first timestamp seen to the
  current one reaches five minutes with no NAS message in between. Never a
  second one in the same recording.
- Any NAS message, at any time before the warning, disables the detector for
  the rest of the recording.
- A backwards jump, or a forward jump of five minutes or more between two
  consecutive messages, restarts the window.
- A recording containing no messages at all cannot trigger it: there are no
  timestamps to measure with. The cell-site panel covers that case live, with
  its "Nothing from the modem" notice.

## Validation

Upstream tested it on a TP-Link with and without a SIM (silent with, warning
without) and on an Orbic without a SIM. During review it fired on an Orbic
with an inactive Verizon SIM that a tower had rejected, which the maintainers
judged correct. Not exercised on this fork's hardware yet.

## Configuration

Off by default. To turn it on, set in the device's config:

```toml
[analyzers]
no_nas_messages = true
```

Or use the switch on the settings page of the web interface.

## Sources

- [EFForg/rayhunter#1132](https://github.com/EFForg/rayhunter/pull/1132),
  the upstream change, and
  [EFForg/rayhunter#882](https://github.com/EFForg/rayhunter/issues/882), the
  request behind it.
- 3GPP TS 24.301, *Non-Access-Stratum (NAS) protocol for Evolved Packet
  System*, for what the missing conversation would consist of.
