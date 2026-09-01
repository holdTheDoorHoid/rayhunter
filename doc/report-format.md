# Report Format

When Rayhunter analyses a recording, it writes an analysis report. This page
documents that report's structure and its version history, for anyone reading a
report outside the interface or writing tools against it. If you only ever view
reports in the dashboard, you do not need this page.

## Shape: newline-delimited JSON

A report is **newline-delimited JSON** (NDJSON): one JSON object per line, not one
big array. Read it a line at a time.

- **The first line is the report metadata.**
- **Every following line is one analysis row**, one message from the recording
  that had something worth writing.

## The metadata line

The first line describes the analysis as a whole:

- `analyzers`, the list of detectors that were active, each with its `name`,
  `description`, and code `version`. This records which detectors, at which
  versions, produced the report.
- `rayhunter`, runtime metadata about the Rayhunter build and the run.
- `report_version`, the format version number (currently **3**).

## The analysis rows

Each subsequent line is one row, describing one message:

- `packet_num`, which message in the recording this row is about, counting from
  1. **Added in version 3** (see below). It is what lets a warning be traced to,
  and jumped to, the exact message that produced it.
- `packet_timestamp`, when that message was seen.
- `skipped_message_reason`, set if the message could not be analysed, saying
  why; absent otherwise.
- `events`, the findings for this message: a list in which each entry is either
  an event or empty (empty entries correspond to detectors that had nothing to
  say about this message). Each event has an `event_type`, `Informational`,
  `Low`, `Medium`, or `High`, and a `message` describing it in plain text.

### Rows that are not written

A row whose events are all `Informational`, and which has no skipped-message
reason, is **not written at all**. This keeps a report to the findings plus the
context on the same messages, rather than a full log of routine traffic. The
consequence for reading a report: absence of a message from the report does not
mean it did not occur, see [Severity, and What It Means](./severity.md).

## Version history

The `report_version` distinguishes formats that have changed over time. Rayhunter
reads the older ones and normalises them when it opens an old report, so you can
open a report made by an earlier version; it writes the current version.

- **Version 0**, the legacy, unversioned era. Reports from before versioning is
  treated as version 0, which lets some known false-positive results from that
  period be told apart from later ones.
- **Version 1**, an earlier structured format, grouping analysis entries and
  skipped-message reasons differently from today. Rayhunter still reads it.
- **Version 2**, rows in roughly the current shape (`packet_timestamp`,
  `skipped_message_reason`, `events`) but **without** `packet_num`.
- **Version 3, the current version.** Adds `packet_num` to each row, so a
  finding can be tied to the exact message that produced it. This is the change
  the [packet explorer](./packet-explorer.md) needed, and carrying it is why the
  explorer advanced the format version. Reports written before version 3
  omit `packet_num`, and Rayhunter fills it as absent rather than failing.

## Compatibility note

Because this fork writes version 3, a report made here carries a field older
versions did not. Whether a different (for example, upstream) version reads it
cleanly depends on that version. The reliable approach when moving recordings
between versions is to **re-analyse the raw recording** on whichever version you
are using, which regenerates the report in that version's own format. [Re-analyzing
Recordings](./reanalyzing.md) covers how, and [Compatibility With
Upstream](./fork/compatibility.md) covers the cross-version picture.

## Where to next

- [The Packet Explorer](./packet-explorer.md), the feature the version-3 field
  serves.
- [Severity, and What It Means](./severity.md), the event types and the
  empty-row rule.
- [REST API](./api-docs.md), the endpoints that serve reports and recordings.
