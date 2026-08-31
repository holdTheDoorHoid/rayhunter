# Documentation standards

This is the contract every page in `doc/` is held to. It exists so that a reviewer can point
at a rule rather than argue about taste, and so that anything generating drafts — a person or
a model — is working to the same target.

Two facts about our readers shape everything below.

**They are frightened, or they would not be here.** Someone installing a surveillance
detector is worried about something. Fear narrows working memory and biases people toward
the first plausible explanation. Writing that is merely accurate but hard to parse will be
misread under those conditions. This is a reason for plain language, not a reason to be
vague.

**They will act on what we write.** A misread warning can mean someone abandons a phone they
needed, or dismisses a real detection, or makes a claim in public they cannot support. Docs
for a detector are safety documentation.

---

## 1. One mode per page

We follow [Diátaxis](https://diataxis.fr/). Every page is exactly one of four kinds, and
mixing them is the most common way our drafts go wrong.

| Mode | Answers | Reader is | Our pages |
|---|---|---|---|
| **Tutorial** | "Take me through it once" | Learning | Quick Start, Your First Warning |
| **How-to** | "How do I do X" | Working | Installation, everyday-use pages |
| **Explanation** | "Why is it like this" | Understanding | Everything in Understanding the problem |
| **Reference** | "What are the exact details" | Checking | Config reference, API, detector specs |

The test: if a page contains both a numbered procedure and three paragraphs of background,
split it. Put the background in an explanation page and link to it from step one.

The one deliberate exception is the detector pages, which use a fixed template (§6) that
moves through explanation into reference. That is allowed because the sections are labeled
and ordered, so a reader can stop at the depth they need.

## 2. Progressive depth, in a fixed order

Every substantial page moves through the same four levels. A reader may stop at any of them
and still have something true and usable.

1. **What you would see** — the observable thing, in the reader's terms. No mechanism.
2. **Why it matters** — the consequence for a person. Still no mechanism.
3. **How it works** — the mechanism, introduced only now.
4. **The precise details** — offsets, spec clauses, code paths, test vectors.

This is Mayer's segmenting principle plus straightforward respect for the reader's time.
Level 1 should be readable by someone who does not know what a base station is. Level 4
should be sufficient for someone reimplementing the detector.

Mark the transition explicitly with a heading. Do not let level 3 vocabulary leak into
levels 1 and 2.

## 3. Teach vocabulary before you use it

Pre-training principle: people cannot learn a process and its terminology at the same time
without one of them suffering.

- The first use of a term in a page gets a definition in the same sentence, or a link to the
  [Glossary](./doc/glossary.md).
- Prefer the plain phrase and give the jargon in parentheses, not the reverse. "the permanent
  number that identifies your SIM card (the IMSI)" — not "the IMSI (a permanent identifier)".
- After first use on that page, use one term consistently. Do not alternate between "base
  station", "tower", and "cell site" for the sake of variety. Pick one per page and hold it.
- Never introduce an acronym you do not use again.

## 4. Language

- Second person. Imperative for instructions: "Run", "Open", "Check".
- Present tense. Active voice. The subject of a sentence about an attack should be the
  attacker or the tower, not "it can be observed that".
- Short sentences. If a sentence has three clauses, it is two sentences.
- No "simply", "just", "obviously", "easy". They tell a stuck reader the problem is them.
- No marketing voice. No exclamation marks outside a legal disclaimer.
- Numbers over adjectives where a number exists. "Fires roughly once per flight" beats
  "occasionally fires".
- Write out what a reader would otherwise have to infer. If a step takes four minutes and
  looks frozen for three of them, say so.

Target reading level for the Start Here and Understanding sections: roughly US grade 9. Not
because our readers are unsophisticated, but because they are reading under stress and often
on a phone screen. Reference sections may be as dense as they need to be.

## 5. Honesty rules

These are non-negotiable, and they exist because a detector that is over-trusted is more
dangerous than no detector.

- **Every detector page states its false positives.** If we do not know them, the page says
  we do not know them. A detector with an empty false-positive section is an unfinished page.
- **Never claim a warning proves surveillance.** Warnings are consistent with a pattern.
  The docs' job is to help a reader distinguish "this is worth investigating" from "I have
  been targeted", and those are different claims.
- **Distinguish tested from untested.** Where a detector has only been exercised against
  synthetic or encoder-generated vectors and never against real traffic — which is true of
  several here — the page says so, in the same visual weight as the rest of the text, not in
  a footnote.
- **Distinguish what we detect from what we could detect.** A passive tool sees what the
  network sends to this device. Say what falls outside that.
- **Cite claims about the world.** Anything about how catchers are used, how common they
  are, or what they are capable of gets a citation to
  [Sources and Further Reading](./doc/references.md). Anything about how *this software*
  behaves gets a path into the codebase.

Appropriate reliance is the goal, not maximum trust (Lee & See, 2004). A reader who has
correctly calibrated when to believe us is better served than one who believes us always.

## 6. The detector page template

Every file under `doc/detectors/` uses these headings, in this order, with no additions:

```markdown
# <Plain-language name>

<One sentence. What this watches for, no jargon.>

## What you would see
## Why it matters
## When it fires harmlessly
## How it works
## Precise behavior
## Configuration
## Sources
```

Rules specific to this template:

- The plain-language name is the H1. The code identifier (`imsi_requested`) appears first
  under **Precise behavior**, not in the title.
- **When it fires harmlessly** comes *before* **How it works** on purpose. A reader deciding
  whether to trust a warning needs the false-positive picture before they need the mechanism,
  and putting it last buries it.
- **Precise behavior** names the source file, the severity, the deduplication rule, and what
  the detector deliberately does not look at.
- **Sources** cites the spec clause and any paper the heuristic derives from.
- The one-sentence summary must match the corresponding entry in
  `daemon/web/src/lib/heuristics.ts`. Those strings are what a user sees when deciding
  whether to switch a detector off; if the doc and the UI disagree, that is a bug in one of
  them.

## 7. Diagrams

- Every diagram has a caption stating its takeaway in words. A diagram that needs the
  surrounding paragraph to be intelligible has failed.
- Label elements in the image itself rather than in a legend below it (contiguity
  principle — split attention costs comprehension).
- Cut decorative graphics. They measurably reduce learning from the material they decorate.
- Message-sequence diagrams for anything involving an exchange between a phone and a tower.
  Prose is a bad medium for describing four messages in order.
- SVG, checked into `doc/`, with source text kept editable. Not screenshots of diagrams.

## 8. Screenshots

- Only when the interface is genuinely hard to describe. They rot faster than any other
  content.
- Never contain real identifiers. Redact IMSI, IMEI, TMSI, and location before committing.
- Caption states what to look at, since a reader does not know where to look.

## 9. Cross-referencing

- Relative links, matching `SUMMARY.md`.
- Link forward from concrete to abstract: a how-to step links to the explanation of why. Do
  not make the reader read the explanation first.
- No orphan pages. Every file in `doc/` is reachable from `SUMMARY.md`.
- `book.toml` sets `create-missing = false`, so a link to a page that does not exist fails
  the build rather than shipping.

## 10. What good looks like

The existing `daemon/web/src/lib/heuristics.ts` in this repo is the model. It explains an
identity-request detection by first saying what your phone normally does, then what a real
network occasionally does, then what the suspicious *pattern* is — and it closes by telling
you it sometimes fires on aircraft coming in to land. That progression, and that willingness
to name the false positive without hedging, is the standard for the whole book.
