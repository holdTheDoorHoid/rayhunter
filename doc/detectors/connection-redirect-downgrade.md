# Redirected to 2G

Watches for a tower that hands your phone off onto an old 2G network.

## What you would see

A High warning that your phone was redirected down to 2G. In the web interface
it appears among a recording's warnings; on the device the status line turns to
the warning colour. If you happened to be watching your phone, you might also
have seen the network label drop from 4G to 2G around the same time — though
that drop happens for innocent reasons too, which is the whole difficulty
below.

## Why it matters

2G is the mobile network technology of the early 1990s, and it carries two
weaknesses a modern network fixed: a 2G phone cannot tell a real tower from a
fake one, and 2G's encryption can be broken outright. Moving your phone onto
2G is therefore rarely the goal in itself — it is the setup, the step that
clears away the protections standing between an attacker and
[turning off your encryption](../concepts/attack-encryption.md) or
[reading your traffic](../concepts/cell-site-simulators.md).
[Downgrading You to Weaker Networks](../concepts/attack-downgrade.md) is the
full explanation; this detector watches for the most direct form of it, a
tower actively telling your phone to go to 2G.

## When it fires harmlessly

This is the section to read before acting on a warning from this detector,
because the harmless cases are common in much of the world:

- **Ordinary coverage management, where 2G is real.** In many countries 2G and
  3G carry everyday traffic, and a network legitimately moves phones onto them
  when 4G coverage thins. There, a redirect to 2G can be entirely routine. In
  the United States, where every major carrier except one has shut 2G down, the
  same event is far more unusual — so **where you are changes what this warning
  means** more than almost any other detector's.
- **The edge of LTE coverage.** Near the boundary of 4G service, a network may
  steer your phone to whatever older network it can still reach. This is honest
  behaviour that produces the same message.

The detector cannot tell an attacker's redirect from a coverage-driven one —
both are the same message with the same 2G destination. What it flags is *being
moved*, not the presence of 2G; judging whether the move was hostile is the
reader's job, and depends heavily on where and when it happened. This
repository does not record a measured false-positive rate for it. In a region
where 2G is in daily use, weigh a warning here accordingly, and look for
corroboration — repetition in one spot, or other detectors firing with it — as
[Reading Warnings Without Panicking](../concepts/interpreting-warnings.md)
describes.

## How it works

When your phone is actively connected, the network can end that connection and
name where the phone should go next — an ordinary tool for balancing load and
managing coverage, from [How Cell Networks Work](../concepts/cell-networks.md).
This detector reads those connection-release messages and checks one thing: the
named destination. When the destination is a 2G (GERAN) network, it raises the
warning. When the release names some other destination, it records that as an
informational note instead, without warning.

It does not try to judge intent, infer coverage, or track your location. It
looks at one field in one kind of message and asks whether you were sent to
2G.

## Precise behavior

- **Code identifier:** `connection_redirect_2g_downgrade`.
- **Source:** `lib/src/analysis/connection_redirect_downgrade.rs`; analyzer
  version 1.
- **Severity:** High when the redirected-carrier destination is GERAN (2G).
  Informational for a connection release naming any other carrier destination.
- **Deduplication:** none; it evaluates each connection-release message on its
  own, so a network that repeatedly releases-and-redirects can produce repeated
  warnings.
- **What it deliberately ignores:** redirects to destinations other than 2G
  (recorded as informational, not warned), and connection releases carrying no
  redirection at all. It reads the release message only; it does not
  cross-check against the reselection priorities that the separate
  [2G/3G Advertised Above 4G](./priority-2g-downgrade.md) detector watches. A
  code comment marks comparing the two as future work.
- **Validation:** inherited from upstream and exercised by the "pushed down
  onto a 2G network" demonstration scenario. No real-capture validation is
  recorded in this repository.

## Configuration

Enabled by default. The key is `connection_redirect_2g_downgrade` under
`[analyzers]`, or the "Pushed down to a 2G network" toggle on the settings
page. Worth leaving on even where 2G is ordinary: the concern it addresses is
being moved to 2G by a tower, not 2G existing, and its warnings are still
information you can weigh against where you were.
[Configuration](../configuration.md) covers applying analyzer toggles.

## Sources

- **The attack.** Lin Huang, "Forcing a targeted LTE cellphone into an
  eavesdropping network" (HITBSecConf), cited in the detector source as its
  basis. Background on why 2G is the target is in EFF's white paper and 2023
  post on platform 2G-disable settings — [Sources and Further
  Reading](../references.md).
- **The protocol.** 3GPP TS 36.331 (E-UTRA RRC): the Connection Release
  message and its `redirectedCarrierInfo`, including the GERAN destination this
  detector keys on.
- **In this book.** [Downgrading You to Weaker
  Networks](../concepts/attack-downgrade.md) for the attack and its geographic
  caveat.
