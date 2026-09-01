# How We Validate Detectors

Throughout the detector pages, one phrase recurs: "never confirmed against real
traffic." This page explains what that phrase means, why several detectors carry
it, and what would make it go away. It exists so that the honesty on those pages
reads as a deliberate policy — a standard applied evenly — rather than as an
apology for unfinished work.

The short version: a detector can be right about the bytes and still unproven in
the world, and this book insists on telling you which of those you have.

## What you would see

On a detector page, a "Validation" line in its Precise behavior section, and
often a plain sentence in its false-positive section. In the [detector
reference](./index.md) table, a validation column. This page is the key to
reading those: what each level of evidence actually establishes.

## Why it matters

A detector you trust too much is more dangerous than no detector, because you
will act on it. [Why Detection Is Hard](../concepts/why-detection-is-hard.md)
explains that there is no ground truth for cell-site simulators — no published
list of where they operated to check warnings against. That absence has a direct
consequence for validation: for most detectors, no one can compute a true
false-positive rate, because doing so would require knowing which past warnings
were real. So instead of a number nobody can honestly provide, this book tells
you the *kind* of evidence behind each detector, and trusts you to weigh it.

## The levels of evidence

There are three, and they establish genuinely different things.

### Reference-encoder vectors

The detector is tested against messages produced by an independent,
authoritative implementation of the protocol — a separate program that encodes
the protocol correctly — and checked that it reads them the way that
implementation wrote them.

This is strong evidence of one specific thing: that the detector reads the bytes
correctly. It is the level that caught a real one-bit error in the LPP work
during development, where a layout derived by hand from the specification had
missed an extension bit and shifted every following field. Vectors from an
independent encoder are the ground truth precisely because they do not share the
mistake being tested for.

What it does **not** establish: that the detector behaves well on a live
network. Real traffic is messier, more varied, and more surprising than any
encoder's output. A detector can read every reference vector perfectly and still
fire on some benign real-world message nobody thought to encode. Reference
vectors prove correctness of reading, not fitness for the wild.

### Demonstration and synthetic round-trips

The detector is exercised by Rayhunter's own demonstration scenarios or
hand-built messages, pushed through the real pipeline from raw bytes to warning.
This confirms the whole chain works end to end — the message is parsed,
delivered to the detector, and produces the expected warning.

Its limit is that the test messages were made by the same project that wrote the
detector. A round-trip proves the plumbing, not that the detector matches what a
real attacker or a real network actually sends.

### Real-capture validation

The detector has been run against a recording of real network traffic known to
contain (or known not to contain) the thing it looks for, and behaved correctly.
This is the level that would let someone speak about real-world behavior with
confidence.

**Several detectors in this book do not have this**, and the honest reason is
practical: the fork's test devices see only LTE, so the 2G location detector has
no real 2G traffic to be tested against, and the LPP location detectors have not
been run against a real network issuing a location request. Where a detector has
only the first one or two levels, its page says so in the same plain language as
everything else.

## Where each detector stands

The [detector reference table](./index.md) is the current summary, but the shape
of it is worth stating here:

- **The fork's location detectors** (LPP request, LPP tracking, RRLP) have
  reference-encoder vectors and demonstration round-trips, but **no real-capture
  validation**. The RRLP one has never seen real 2G traffic at all.
- **The inherited upstream detectors** (identity, the two downgrades, the two
  null-cipher checks, incomplete SIB) have demonstration coverage and real-world
  history in the upstream project, but this repository does not record a specific
  real-capture validation, so this book does not claim one for them.

Neither of those is a reason to switch a detector off. It is a reason to weigh
its warnings with the [interpretation
method](../concepts/interpreting-warnings.md) rather than treating any of them as
proof.

## The precise details

- **Where the vectors live.** For the location detectors, the reference vectors
  are checked into the analyzer test modules as hex constants
  (`lib/src/analysis/lpp.rs`, `lib/src/analysis/rrlp.rs`), each round-tripped
  through its own decoder before being trusted. The encoders that produced them
  (pycrate's implementations of 3GPP TS 36.355, TS 44.018 and TS 44.031) are
  offline, throwaway tools, not a runtime dependency.
- **Why hand-derived layouts are checked this way.** The project's working notes
  record the lesson directly: deriving a byte layout from the specification by
  memory is error-prone, and the fix is to verify against an independent encoder
  rather than to trust the derivation. The one-bit error above is the standing
  example.
- **What would upgrade a detector's status.** A real capture containing the
  relevant traffic, run through the detector with the result checked. For the
  location detectors, a recording from a network that actually issues LPP or
  RRLP location requests would move them from "reference-encoder" toward
  "real-capture." Contributions of such captures are the single most valuable
  thing for these pages.
- **The policy behind the pages.** This book's writing standard (`STYLE.md` §5)
  requires every detector page to distinguish tested from untested behavior in
  the same visual weight as the rest of the text, not in a footnote. This page
  is the reference that standard points to.

## Where to next

- [Detector Reference](./index.md) for the per-detector validation column.
- [Writing a New Detector](./writing-a-detector.md) for how the test vectors and
  demo scenarios are built.
- [Reading Warnings Without
  Panicking](../concepts/interpreting-warnings.md) for how validation status
  feeds into trusting a warning.
