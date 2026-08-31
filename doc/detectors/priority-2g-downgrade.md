# 2G/3G Advertised Above 4G

Watches for a tower that advertises 2G or 3G networks as higher-priority
choices than the 4G towers around it.

## What you would see

A High warning that a tower ranked an old network above nearby 4G. It appears
among a recording's warnings and turns the device status line to the warning
colour. Unlike a redirect, nothing visible happens on your phone at the moment
this fires — the tower is broadcasting misleading directions, and an idle phone
may act on them later without any single visible event.

## Why it matters

This is the quieter cousin of a redirect. Instead of telling one phone to move,
a tower broadcasts a list of neighbouring networks with priorities, and idle
phones nearby use that list to decide which network to prefer. Recall from
[How Cell Networks Work](../concepts/cell-networks.md) that these broadcasts
are unencrypted and carry no signature, so anyone with the right equipment can
transmit their own. Advertising weak 2G or 3G networks as the *preferred*
choice is a way to herd many idle phones down onto a network where they can be
intercepted — without ever sending a message to any particular phone. A
properly configured tower ranks its modern 4G neighbours highest.
[Downgrading You to Weaker Networks](../concepts/attack-downgrade.md) is the
full picture; this detector watches the broadcast-priority form of it.

## When it fires harmlessly

- **Genuinely unusual but honest network layouts.** A real tower in an area
  with patchy 4G might legitimately rank an older network highly because it is
  the best fallback actually available there. The priorities that look like an
  inversion can reflect real coverage.
- **Regions where 2G and 3G are working networks.** As with the other
  downgrade detector, the meaning of this warning depends on where you are. In
  much of the world old networks carry ordinary traffic, and a tower giving
  them real priority is less remarkable than it would be somewhere they have
  been shut down.

This detector has a specific history worth knowing, because it is a case of the
honesty this book asks for working as intended. **Earlier versions raised many
false alarms.** The current version (2) is stricter: it separates a true
inversion — an old network ranked *above a 4G neighbour that is actually
present* — from the much weaker case of a tower that advertises old neighbours
but lists no 4G neighbour at all. The first still warns at High; the second is
now only an informational note, because a cell with no 4G neighbours to list is
not necessarily doing anything wrong. That change removed a large source of
noise. Even so, no measured false-positive rate is recorded for the current
version, so treat a warning as a lead to corroborate, not a conclusion.

## How it works

Towers broadcast their neighbour information in numbered blocks (the *System
Information Blocks* of [How Cell Networks Work](../concepts/cell-networks.md)).
Different blocks carry different neighbours: the LTE (4G) neighbours and their
priorities in some blocks, the 3G neighbours in another, the 2G neighbours in
another. This detector reads them across one broadcast cycle and keeps track of
two things: the highest priority given to any 4G neighbour, and the highest
priority given to any old (2G or 3G) neighbour.

At the end of the cycle it compares them. If an old network was ranked above
the best 4G neighbour, that is the inversion an attacker wants, and it warns. If
old neighbours were advertised but no 4G neighbour was seen at all, it records
the weaker informational note described above rather than a warning. Then it
resets and watches the next cycle.

## Precise behavior

- **Code identifier:** `lte_sib6_and_7_downgrade`.
- **Source:** `lib/src/analysis/priority_2g_downgrade.rs`; analyzer version 2.
- **Severity:** High when the highest legacy (2G/3G) reselection priority
  exceeds the highest LTE priority seen in the same cycle. Informational when
  legacy neighbours are advertised but no LTE priority was seen at all.
- **Deduplication:** per broadcast cycle. Priority state is reset each time a
  fresh cycle begins (marked by a SIB1), so it reports at most once per cycle
  rather than once per block.
- **What it deliberately ignores:** it compares only the highest priorities, not
  the full neighbour lists, and it does not correlate with the active
  [redirect detector](./connection-redirect-downgrade.md). A code comment marks
  tracking full reselection state across the two as future work.
- **Validation:** inherited from upstream and exercised by the "2G advertised as
  a better choice than nearby 4G" demonstration scenario. No real-capture
  validation is recorded in this repository. The version-2 refinement described
  above is the documented response to the earlier false-alarm history.

## Configuration

Enabled by default. The key is `lte_sib6_and_7_downgrade` under `[analyzers]`,
or the "Old networks advertised as better than nearby 4G" toggle on the
settings page. [Configuration](../configuration.md) covers applying analyzer
toggles.

## Sources

- **The heuristic.** Derived from heuristic T7 in Shinjo Park's "Why We Cannot
  Win," cited in the detector source. Background on downgrade attacks is in
  EFF's white paper and 2023 post — [Sources and Further
  Reading](../references.md).
- **The protocol.** 3GPP TS 36.331 (E-UTRA RRC): System Information Blocks and
  the cell-reselection priorities carried in SIB3, SIB5 (LTE), SIB6 (3G) and
  SIB7 (2G).
- **In this book.** [Downgrading You to Weaker
  Networks](../concepts/attack-downgrade.md).
