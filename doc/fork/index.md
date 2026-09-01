# Why This Fork Exists

Rayhunter is a project of the Electronic Frontier Foundation. This book
documents a *fork* of it — a version that carries a set of additional features
on top of EFF's, maintained separately for now. This page explains what that
means for you and why the fork exists, so you can decide whether it is the
version you want.

## The short version

The upstream Rayhunter is deliberately focused: EFF keeps it lean and accepts
changes carefully, one feature at a time, so that a tool people rely on for
safety stays small and reviewable. That is a good way to run it, and nothing
here is a complaint about it.

This fork exists because, in the course of using and developing Rayhunter, its
maintainer built a number of features that were useful enough to want all at
once — improvements to the web interface, the device display, the recordings,
and the detectors — faster than any single feature could work its way through
upstream review. Rather than wait, the fork carries them together, while each
one is offered back to upstream individually on its own merits.

So the honest framing is: this is not a competitor to EFF's Rayhunter, and it
is not a criticism of it. It is a place where several improvements live together
while they make their separate way home.

## What that means for you

- **Everything upstream Rayhunter does, this does too.** The fork is built on
  top of EFF's work, not in place of it. The core detection, the supported
  devices, and the basic workflow are the same.
- **It adds features upstream does not have yet.** The web interface has more
  panels, the device display is more configurable, recordings can be named and
  rotated, there is a packet explorer, and there are location-tracking detectors
  that upstream does not carry. [What It Adds](./features.md) is the full list,
  grouped by what you are trying to do.
- **Some of those features may converge with upstream over time, and some may
  not.** Each is at a different stage of being offered back. [Compatibility With
  Upstream](./compatibility.md) is the page to read if you need to know whether
  you can move between the two versions, and [Contributing
  Upstream](./upstreaming.md) explains how features are proposed.

## The honesty this fork owes you

Two things are worth stating plainly, because they affect whether you should
depend on this fork:

- **Features here are at different stages with upstream.** Some have a
  maintainer's encouragement and are close to being proposed. Some have been
  proposed before and were closed. Some overlap with work another contributor
  has claimed. A feature being in this fork does not mean it will ever be in
  upstream, and where the status is known, the [feature list](./features.md) and
  [Differences You Will Notice](./differences.md) say so.
- **The added detectors carry their own caveats.** The location-tracking
  detectors that are unique to this fork have not been confirmed against real
  network traffic — a limitation documented on each detector page and in [How We
  Validate Detectors](../detectors/validation.md). They are a genuine addition,
  and an unproven-in-the-wild one, and this book will not pretend otherwise.

## Where to next

- [What It Adds](./features.md) — the features, grouped by goal.
- [Differences You Will Notice](./differences.md) — what changes if you are
  coming from upstream Rayhunter.
- [Compatibility With Upstream](./compatibility.md) — configs, recordings, and
  whether you can switch back.
