# Contributing Upstream

This fork is meant to feed back into EFF's Rayhunter, one feature at a time.
This page explains how that works, for anyone who wants to help move a feature
upstream or to understand where the pieces stand. It is a summary; the
authoritative, feature-by-feature record lives in the repository's `UPSTREAM.md`,
and this page links there rather than repeating it.

## The guiding rule: one feature per pull request

Upstream Rayhunter accepts changes one feature at a time, so that a safety tool
stays small and reviewable. That single rule shapes everything about how this
fork is organised. The fork carries many features together for its own users,
but each one has to be able to *leave* on its own, cleanly separable into a
single pull request that stands alone.

Because of that, this fork does not expect anyone to "merge the fork" upstream.
The path is always: take one feature, prepare it as its own change against
upstream's current code, and propose that.

## Talk to the maintainers first

The upstream project's `CONTRIBUTING.md` asks that, for anything beyond a small
documentation fix, you check existing issues and **discuss with the maintainers
before implementing**. This is not a formality, at least one feature in this
fork was built and then met resistance upstream that earlier discussion would
have surfaced. If you are picking up a feature to propose, start on the relevant
issue, not with a finished pull request.

## How features are tracked

So that features can be found and separated later, commits in this fork carry two
trailers naming the feature and any related upstream issue:

```
Feature: keep-screen-on
Upstream: EFForg/rayhunter#916, EFForg/rayhunter#539
```

This lets every commit belonging to one feature be listed together, which is
what makes building a single-feature pull request from a fork with many features
tractable. A standalone bug fix gets its own feature name too, so it can be
offered separately, fixes are the easiest thing to get merged and should not be
held back by the feature that uncovered them.

## Where each feature stands

`UPSTREAM.md` groups the fork's features by their status with upstream, and that
grouping is the honest picture worth knowing before you depend on or propose any
of them:

- **Ready to propose**, has an upstream issue and encouragement from a
  maintainer.
- **Already handled upstream**, the fork has it because it branched after it
  landed; nothing to send.
- **Claimed by someone else**, another contributor has told upstream they are
  working on it; coordinate rather than duplicate.
- **Proposed before and closed**, was taken upstream and rejected; know the
  objection before trying again.
- **No issue yet**, worth opening one and discussing before building further.

For which feature is in which group, and the exact files and commits each one
touches, read `UPSTREAM.md` in the repository. It is kept current as features
move between these states.

## A note on pushing

Contributing to upstream means opening pull requests against EFF's repository,
which is a deliberate, human decision each time, this fork's tooling does not
push to upstream on its own, and its upstream remote is configured to prevent
accidental pushes. If you maintain your own copy of this fork, keep that
safeguard.

## Where to next

- `UPSTREAM.md` in the repository, the per-feature status and file lists.
- The upstream project's `CONTRIBUTING.md`, the process and expectations.
- [What It Adds](./features.md), the features as users see them, if you are
  deciding which to help with.
