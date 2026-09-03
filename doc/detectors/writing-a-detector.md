# Writing a New Detector

This page is for contributors adding a detector to Rayhunter. It walks through
every place a new analyzer has to be wired in, in the order that compiles
cleanly at each step, and states the evidence a detector needs before its page
can honestly describe it.

It assumes you have read [How We Validate Detectors](./validation.md), because
the test-vector expectation at the end is not optional, it is the difference
between a detector this book can stand behind and one it cannot.

## Before you write code

A detector is a safety claim. Two things are worth settling first:

- **What message carries the evidence, and can Rayhunter see it?** Rayhunter
  reads the control messages its own modem logged, LTE RRC and NAS, and now 2G
  Layer 3 bytes. If your signal lives somewhere Rayhunter does not parse, the
  work starts there, not in a new analyzer.
- **What is the honest false-positive story?** If you cannot describe when your
  detector fires harmlessly, the detector is not ready, and its page (which
  [STYLE.md](../../STYLE.md) §5 requires to state false positives) cannot be
  written. Work this out before, not after.

## The Analyzer trait

Every detector implements one trait, `Analyzer`, defined in
`lib/src/analysis/analyzer.rs`. It has four required methods and one optional
one:

- `get_name`, a short, human name.
- `get_description`, what it looks for, the events it can raise, and its
  false-positive conditions.
- `analyze_information_element`, the heart of it: given one parsed message (an
  `InformationElement`), its packet number and its timestamp, optionally
  return an `Event`.
- `get_version`, a number you increase whenever you materially change the
  heuristic, so old and new reports can be told apart.
- `report_skipped_packet`, optional: called with the timestamp of every packet
  that produced nothing to analyze, so a detector that watches the clock (the
  [no-NAS detector](./no-nas-messages.md) is one) still sees time pass. The
  default does nothing. Use the recording's timestamps, never the system
  clock, so re-analysing a recording gives the same answer.

An `Event` carries a severity (`EventType`: `Informational`, `Low`, `Medium`,
`High`) and a message string. Keep any per-message state small: an analyzer may
run over thousands of messages alongside many others.

One rule that shapes severity choices: **a report row whose events are all
`Informational` is never written** (`AnalysisRow::is_empty`). A detector that
only ever emits `Informational` events is invisible in a report on its own. If
your detector needs to be seen, its meaningful events must be at least `Low`.
[Severity, and What It Means](../severity.md) explains why.

## The steps, in build-clean order

1. **Create the module.** Add `lib/src/analysis/my_detector.rs` with your struct
   and its `Analyzer` implementation.

2. **Declare it.** Add `pub mod my_detector;` to `lib/src/analysis/mod.rs`.

3. **Add a config switch.** In `AnalyzerConfig` (in `analyzer.rs`), add a
   `pub my_detector: bool` field, and set its default in the `Default` impl.
   Default to `true` for a real detector, the config test
   `analyzers_missing_from_an_old_config_default_on` checks that a detector
   absent from an older config file comes up enabled, so an update never
   silently disables one.

4. **Wire it into the harness.** In `Harness::new_with_config` (also in
   `analyzer.rs`), add a block that pushes your analyzer when its config flag is
   set, matching the others.

5. **Add it to the shipped config.** Add `my_detector = true` under
   `[analyzers]` in `dist/config.toml.in`. Keep new keys inside that table (a
   top-level key appended after it is silently parsed into `[analyzers]`).

6. **Write the settings-page entry.** Add a `HeuristicInfo` to
   `daemon/web/src/lib/heuristics.ts` with your key, a plain title, a
   one-sentence `summary`, and the `detects`, `matters`, and `noise` fields.
   This is enforced: the type derives its keys from the config, so a detector
   with no entry here is a **type error**, not a silent omission. The `summary`
   is what a user reads when deciding whether to switch your detector off, and
   your detector page's one-sentence summary must match it. Two more places
   know the key by name: the `AnalyzerConfig` interface in
   `daemon/web/src/lib/utils.svelte.ts`, which the type above derives from,
   and the expected key list in `heuristics.spec.ts`, whose tests also hold
   the `summary` to 160 characters so it fits under a checkbox. Run
   `npm run check` and `npm run test` in `daemon/web` after adding the entry.

7. **Add a demonstration scenario.** In `daemon/src/demo.rs`, add a scenario
   that injects a message which makes your detector fire, through the real
   parsing pipeline. This is what lets someone see your detector work on purpose
   (see [Your First Warning](../first-warning.md)) and is part of its
   validation.

8. **Write the tests.** Covered next, this is the part that cannot be skipped.

## The test-vector expectation

A detector that decodes a protocol must be tested against messages produced by
an **independent reference encoder**, not against bytes you wrote by reading the
same specification your detector reads. This is not a style preference; it is the
check that caught a real one-bit error in the LPP work, where a hand-derived
layout had missed an extension bit ([How We Validate
Detectors](./validation.md) tells that story).

The pattern the existing location detectors follow:

- Generate encoded messages with an authoritative implementation of the
  protocol (the LPP and RRLP detectors used pycrate's implementations of the
  relevant 3GPP specifications), and round-trip each through that
  implementation's own decoder before trusting it.
- Check the encoded bytes in as hex constants in your analyzer's test module.
  The encoder itself stays an offline, throwaway tool, not a runtime
  dependency.
- Only decode fields by hand that sit at a fixed offset, with no
  variable-length content in front of them, and stop at the first field you
  cannot size. Failing to `None` rather than guessing matters: a wrong alignment
  that reports the wrong message is worse than an honest "could not read it."
- Add a truncation test that feeds progressively shorter inputs and asserts the
  detector never panics. Some of this traffic is attacker-shaped; robustness is
  part of correctness.

If your detector cannot yet be tested against real captures, which is true of
several here, because the test devices see only LTE, that is acceptable, but its
page must say so plainly. It cannot claim real-world validation it does not have.

## Keep the docs in step

Two artifacts must move together with the code, and the pull-request checklist
should say so:

- **`heuristics.ts` and the detector's doc page.** The one-sentence summaries
  must match; a header comment in `heuristics.ts` already says the two move
  together.
- **A new detector page** under `doc/detectors/`, using the [STYLE.md
  §6](../../STYLE.md) seven-heading template, plus a row in the [detector
  reference table](./index.md) and an entry in `SUMMARY.md`. Because the book
  sets `create-missing = false`, a `SUMMARY.md` link with no file fails the
  build, so the page and the summary entry land together.

## Where to next

- [How We Validate Detectors](./validation.md), the evidence standard in full.
- [Detector Reference](./index.md), the existing detectors as worked examples;
  the LPP and RRLP sources are the model for reference-vector testing.
- [How Detection Works](../heuristics.md), the shared background on how messages
  reach an analyzer.
