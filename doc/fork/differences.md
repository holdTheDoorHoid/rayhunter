# Differences You Will Notice

If you are coming from upstream Rayhunter, or thinking about moving between the
two, this page describes what actually looks and behaves differently, in both
directions. It is about what you would notice as a user, not about the code.
For whether your configs and recordings carry over, which is a separate and more
urgent question, see [Compatibility With Upstream](./compatibility.md).

## Moving from upstream to this fork

The core is the same: the same detection you already rely on, the same devices,
the same basic workflow of installing, recording, and reviewing. What you gain
is additional, and mostly visible in three places.

**In the web interface**, you will see more than upstream shows: a panel for the
serving cell and its neighbours, warning counts broken out by severity, live
system health, plain-language explanations under each detector, a dark mode, and
a configuration page organised into sections. There is a [packet
explorer](../packet-explorer.md) for browsing the actual messages in a recording,
which upstream does not have. If you turn it on, the interface can also show your
device's own identity, and you can put a password on the interface, both off by
default.

**On the device screen**, you can change the colour of each state, the height of
the status line, whether the screen stays on, and what a button press does. None
of these changes is required; the defaults behave much as upstream does.

**In detection**, this fork adds detectors upstream does not carry, the
[LPP](../detectors/lpp.md) and [RRLP](../detectors/rrlp.md) location detectors, and records 2G and 3G traffic into captures instead of dropping it. The added
detectors come with an honest caveat, repeated from their pages: they have not
been confirmed against real network traffic. They are on by default, and a
warning from them should be read with that in mind.

**In recordings**, you can name them, add notes, have them rotate automatically
by size or time, and have clean ones auto-deleted when space is low. A named
recording is never auto-deleted.

## Moving from this fork back to upstream

The most important difference in this direction is not a feature but a file
format. This fork's [packet explorer](../packet-explorer.md) advanced the
analysis report format to a newer version. A recording analysed here therefore
carries a report format that upstream may not read the way you expect. This is
the detail that matters in a hurry, and [Compatibility With
Upstream](./compatibility.md) covers it precisely, read it before you move
recordings between versions.

Beyond that, switching back means losing the additions above: the extra panels,
the packet explorer, the configurable display, named and rotating recordings,
the location detectors, and optional web accounts. Any settings unique to this
fork stop having an effect. Your captures themselves, the raw radio
recordings, are not the fork's invention and are not affected by the switch;
it is the *analysis report* format to be careful about.

## Where features stand with upstream

Because this fork offers its features back one at a time, they are at different
stages, and a few differences are worth calling out honestly:

- **Some features have been proposed to upstream and closed.** The
  keep-the-screen-on feature, for example, was proposed upstream and rejected
  once; this fork's version answers part of the objection (the "only while
  plugged in" option) but the rest of that discussion is unsettled. If you
  depend on such a feature, know that it may not converge with upstream.
- **Some overlap with work another contributor has claimed.** Parts of the
  cell-site information, and the location detectors, touch areas where someone
  else has told upstream they are working. The approaches differ, and how they
  resolve is not decided.
- **Some are likely to converge.** Straightforward fixes and clearly useful
  additions with a maintainer's encouragement are the ones most likely to end up
  in upstream, at which point the difference disappears.

[Contributing Upstream](./upstreaming.md) explains the process behind this, and
the repository's `UPSTREAM.md` is the maintainer-level record of exactly where
each feature stands.

## Where to next

- [Compatibility With Upstream](./compatibility.md), the precise, in-a-hurry
  answer on configs, recordings, and report formats.
- [What It Adds](./features.md), the full feature list if you have not seen it.
