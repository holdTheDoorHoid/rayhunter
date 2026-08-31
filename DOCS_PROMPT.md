# Documentation generation prompts

Run these from the repo root with **Claude Code**, so the model reads actual source rather
than recalling upstream Rayhunter from training data. That distinction matters more here than
in most projects: this fork has diverged substantially, and a model that pattern-matches to
upstream will confidently document features you changed.

**Preconditions for every prompt below.** Paste this block at the start of a session, once:

> You are writing documentation for this repository, a fork of EFForg/rayhunter. Before you
> write anything:
>
> - Read `STYLE.md`. It is binding, not advisory. Sections 5 (honesty rules) and 2
>   (progressive depth) are the ones drafts most often violate.
> - Read `UPSTREAM.md`. It is the authoritative inventory of what this fork changes.
> - Read `doc/references.md`. Every claim about the world cites something there. Do not
>   invent citations, do not cite a paper you have not been given, and do not attribute a
>   claim to a source unless you can point to where it says that.
> - Treat the codebase as the source of truth for behavior, and the specs and papers as the
>   source of truth for context. Where they conflict, document the code's actual behavior and
>   flag the conflict.
>
> When you cannot determine something from the repo, write
> `<!-- NEEDS INPUT: specific question -->` inline. Do not guess, and do not smooth over a
> gap with plausible-sounding prose. A draft with ten honest gaps is more useful to me than
> one with ten invented details.

---

## Phase 0 — Inventory

Run once. Do not skip it; every later prompt depends on its output.

> Build an inventory of this repository for documentation purposes. Produce a single markdown
> file, `doc/INVENTORY.md`, containing:
>
> 1. **Detectors.** Every analyzer in `lib/src/analysis/`. For each: code identifier,
>    display name, severity levels it emits, what message types it examines, its
>    deduplication rule, and whether its tests use real captures, encoder-generated vectors,
>    or synthetic data. Note explicitly which have never been exercised against real traffic.
> 2. **Configuration.** Every key readable from `config.toml`, with type, default, where it
>    is read in the code, and what breaks if it is wrong.
> 3. **HTTP surface.** Every route, its method, auth requirement, and response shape.
> 4. **Device support.** Every device with a module under `daemon/src/display/` or an
>    installer path, and what is device-specific about each.
> 5. **Fork delta.** Cross-reference `UPSTREAM.md` against the actual diff
>    (`git diff upstream/main --stat`). List anything in the diff that `UPSTREAM.md` does not
>    account for, and anything `UPSTREAM.md` claims that you cannot find in the code.
> 6. **Undocumented behavior.** Anything user-visible with no corresponding page in
>    `doc/SUMMARY.md`.
> 7. **Open questions.** Everything you could not resolve from the repo alone.
>
> Item 5 and item 7 are the reason for this pass. Be exhaustive about both.

**Answer the open questions before continuing.** This is the single highest-leverage step in
the whole process.

---

## Phase 1 — Explanation pages (the "why")

These are the hardest pages and the ones with the most value, so they go first: every other
page links into them, and writing them first prevents the how-to pages from silently growing
their own inconsistent explanations.

Do these **one at a time**, in `SUMMARY.md` order, because each assumes the vocabulary
established by the previous one.

> Write `doc/concepts/<FILE>.md`.
>
> This is an **explanation** page in the Diátaxis sense: it exists to build understanding,
> not to get a task done. No numbered procedures. If you find yourself writing "first, run",
> you are on the wrong page.
>
> **Reader model.** Someone who owns a phone and is worried about surveillance. Assume no
> knowledge of cellular networks, radio, or cryptography. Assume normal adult intelligence
> and genuine motivation — they are reading this because it matters to them, so do not
> condescend and do not pad. Assume they may be reading on a phone, under stress.
>
> **Structure.** Follow `STYLE.md` §2 strictly: what you would see, why it matters, how it
> works, precise details — in that order, with headings marking each transition. Someone who
> reads only the first section must come away with something true.
>
> **Grounding.** Every factual claim about how networks or attackers behave cites a source
> from `doc/references.md`, by name inline (e.g. "EFF's 2019 white paper describes...") plus
> a relative link. Prefer the EFF white paper for mechanism, the academic papers for
> measured results and taxonomy, and the 3GPP specs for what the protocol actually mandates.
> Where our behavior differs from what a spec says should happen, say so — that gap is often
> the whole point of the page.
>
> **Concrete before abstract.** Open with a situation, not a definition. "Your phone is in
> your pocket on a train" beats "Cellular networks are organized into cells." The definition
> comes after the reader has something to attach it to.
>
> **One diagram maximum**, described in a fenced block marked `<!-- DIAGRAM: ... -->` with
> the elements, their labels, and the caption. I will produce the SVG separately. Only
> propose one where a sequence of messages or a spatial relationship is genuinely hard to
> convey in prose.
>
> Length: whatever the idea needs. These pages may be long. They may not be padded.

### Phase 1b — the trust-calibration page

`doc/concepts/interpreting-warnings.md` gets its own prompt because it is the page most
likely to cause harm if written badly.

> Write `doc/concepts/interpreting-warnings.md`.
>
> Purpose: help a reader form an accurate sense of when to believe a warning. Both failure
> modes are real and both are harmful — someone who treats every warning as proof of
> targeting, and someone who stops reading warnings because the tool cried wolf.
>
> Cover, in the reader's language and without lecturing:
>
> - What a warning is and is not. It says a pattern consistent with certain attacks appeared
>   in traffic this device saw. It is not proof, and it does not identify who.
> - Why base rates matter. If catchers are rare where you are, most warnings are something
>   else — and that is true even for a detector that is right most of the time. Give a
>   worked numerical example with concrete numbers. Keep the arithmetic visible.
> - What raises confidence: repetition at the same place, several independent detectors
>   firing together, correlation with a known event, a clean capture others can verify.
> - What lowers it: single low-severity events, known-noisy detectors, aviation and border
>   crossings, unfamiliar networks while roaming, lab and test environments.
> - What to actually do at each severity level. Concrete actions, not "consider consulting an
>   expert."
> - How to talk about a finding without overstating it — for journalists, organizers, and
>   anyone who might repeat this publicly. Give example phrasings that are defensible and
>   example phrasings that are not.
>
> Do not reassure and do not alarm. The tone to aim for is a competent colleague explaining
> what the instrument can and cannot support, respecting that the decision is the reader's.
>
> Cite the White-Stingray paper for the finding that detectors implementing the same methods
> disagree with each other, and the false-positive notes in our own detector pages.

---

## Phase 2 — Detector pages

One page per analyzer, one prompt per page.

> Write `doc/detectors/<NAME>.md` for the analyzer at `lib/src/analysis/<FILE>.rs`.
>
> Use the template in `STYLE.md` §6 exactly — those seven headings, in that order, nothing
> added.
>
> Read before writing: the analyzer source and its tests, its entry in
> `daemon/web/src/lib/heuristics.ts`, its config key in `dist/config.toml.in`, the relevant
> section of `doc/heuristics.md`, and any note about it in `UPSTREAM.md`.
>
> Section-specific requirements:
>
> - **What you would see** — the user-visible warning text and where it appears. No protocol
>   vocabulary at all in this section.
> - **Why it matters** — the consequence to a person, tied to a documented attack from
>   `doc/references.md`. Say which attack and cite it.
> - **When it fires harmlessly** — required, and it must be specific. Pull known cases from
>   the code comments, the `heuristics.ts` entry, and upstream issues. If the honest answer
>   is that we do not know the false-positive rate, write that sentence plainly. Do not leave
>   this section thin to make the detector look good.
> - **How it works** — the mechanism at a level a curious non-specialist can follow. Name
>   the message types in plain terms first, protocol names second.
> - **Precise behavior** — code identifier, source path, severity, dedup rule, what it
>   deliberately ignores, and how it was validated. State explicitly whether it has been run
>   against real traffic. For the LPP and RRLP analyzers in particular, `UPSTREAM.md` records
>   that they were verified against pycrate reference encoders but not against real captures
>   from a live 2G network — say so.
> - **Sources** — 3GPP clause and any paper the heuristic derives from.
>
> Verify the one-sentence summary against `heuristics.ts`. If they disagree, stop and tell me
> which you think is wrong rather than silently picking one.

---

## Phase 3 — Tutorials

Only two, and they carry disproportionate weight: they are where a new user either succeeds
or gives up.

> Write `doc/quick-start.md`.
>
> This is a **tutorial**, not a how-to. The difference: a tutorial guarantees a successful
> outcome for a reader who follows it exactly, and it is allowed to make choices on the
> reader's behalf to achieve that. Pick one device and one path. Send everyone else to the
> how-to pages.
>
> Requirements, following Carroll's minimalist principles:
>
> - Begin with a real task, not with setup theory. The reader should be doing something by
>   the end of the first screen.
> - Every step states its expected result, so the reader can self-check without waiting for
>   a failure three steps later.
> - Every step that can plausibly fail gets an inline recovery note: what it looks like when
>   it goes wrong, and what to do. Do not defer these to a troubleshooting page; the reader
>   who needs them is stuck right now.
> - Say how long slow steps take, and what a frozen-looking screen means.
> - End at a defined success state the reader can recognize, and tell them what to read next.
> - No branching. Where the path forks, choose, and link the alternative.

> Write `doc/first-warning.md`.
>
> A tutorial for the moment a reader gets their first warning. This is the emotional peak of
> using this tool and the point of maximum risk of misinterpretation.
>
> Walk them through: finding the warning, reading what it says, opening the recording,
> checking which detector fired and at what severity, using the packet explorer to see the
> actual messages, and deciding what it means. Use the demo feature (`demo.rs`) so the reader
> can produce a warning on purpose and practice this before it happens for real — check how
> the config gate works before writing the steps.
>
> Route to `doc/concepts/interpreting-warnings.md` for interpretation rather than repeating
> it here.

---

## Phase 4 — Fork pages

> Write the pages under `doc/fork/`.
>
> `UPSTREAM.md` is your source, but it is written for maintainers deciding what to propose
> upstream. These pages are for users. Translate: a user does not care which commit a feature
> lives in, they care what the software does that upstream's does not, and whether that
> affects them.
>
> - `index.md` — why this fork exists, in a few honest paragraphs. Frame it as this fork's
>   maintainers describe it, not as criticism of upstream. Note that features here are being
>   offered upstream individually.
> - `features.md` — what this adds, grouped by what the user is trying to do rather than by
>   subsystem. Each entry: one plain sentence, then a link to its full page.
> - `differences.md` — what a user of upstream will notice on switching. Both directions.
> - `compatibility.md` — config compatibility, recording and report format compatibility
>   (note the report format version bump the packet explorer carries), and whether they can
>   switch back. This is the page people will need in a hurry; be precise.
>
> Where `UPSTREAM.md` records that a feature was proposed upstream and rejected, or is
> claimed by another contributor, say so plainly. Users deciding whether to depend on this
> fork deserve to know which parts are likely to converge with upstream and which are not.

---

## Phase 5 — Consistency and review

> Read every file in `doc/`. Report without fixing:
>
> 1. Terminology drift — the same concept named differently across pages. List each concept
>    and the competing terms with file locations.
> 2. Any claim about the world with no citation, and any citation not in
>    `doc/references.md`.
> 3. Any detector page with a thin or missing false-positive section.
> 4. Any page where level-3 vocabulary appears in level-1 or level-2 sections
>    (`STYLE.md` §2).
> 5. Broken relative links, pages missing from `SUMMARY.md`, and pages in `SUMMARY.md`
>    without files.
> 6. Contradictions in paths, ports, defaults, or command syntax between pages.
> 7. Remaining `NEEDS INPUT` markers.
> 8. Any place where the docs and `daemon/web/src/lib/heuristics.ts` disagree.

Then a separate pass, which needs a different mindset:

> Re-read the Start Here and Understanding sections as a hostile reviewer with two specific
> objections. First: where does this overstate what the tool can prove? Second: where would a
> frightened reader plausibly misread this as saying they have been targeted? Quote the exact
> sentences. Do not soften them yet — I want the list.

---

## Phase 6 — Readability check

Mechanical, and worth running because stressed readers on small screens are the actual
audience.

```bash
pip install --break-system-packages textstat
python3 - <<'EOF'
import glob, re, textstat
for f in sorted(glob.glob('doc/**/*.md', recursive=True)):
    t = open(f).read()
    t = re.sub(r'```.*?```', '', t, flags=re.S)      # drop code blocks
    t = re.sub(r'\[([^\]]*)\]\([^)]*\)', r'\1', t)   # unwrap links
    t = re.sub(r'<!--.*?-->', '', t, flags=re.S)     # drop comments
    if len(t.split()) < 100: continue
    g = textstat.flesch_kincaid_grade(t)
    flag = '  <-- CHECK' if g > 12 else ''
    print(f'{g:5.1f}  {f}{flag}')
EOF
```

Grade 12+ in Start Here or Understanding means rewrite. Reference and detector pages are
allowed to be dense. Treat the number as a smoke alarm, not a target — optimizing prose to
hit a score produces worse writing.

---

## Working notes

- **One page per session** where you can. Long sessions drift from `STYLE.md`; a fresh
  session that re-reads it produces more consistent output.
- **Commit each page separately** with the page name in the subject. Makes review tractable
  and reverting cheap.
- **Read the draft rendered**, not raw. `mdbook serve --open` from the repo root. Structural
  problems are obvious in a browser and invisible in a diff.
- **The false-positive sections are the ones to review hardest.** They are where a model is
  most likely to be agreeable rather than accurate, and where being wrong does the most
  damage to a reader who is deciding whether to trust a warning.
- **Keep `heuristics.ts` and the detector pages in sync.** Its header comment already says
  the two must move together. Add that check to the pull request checklist.
