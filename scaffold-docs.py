#!/usr/bin/env python3
"""
Create stub pages for the new documentation structure.

Run from the repo root. Existing files are never overwritten. Each stub carries its
own brief as an HTML comment, so a per-page generation session has the context in
the file it is being asked to write.

    python3 scaffold-docs.py [--dry-run]
"""
import os
import sys

DRY = "--dry-run" in sys.argv

# path: (title, mode, brief)
PAGES = {
"what-this-does.md": ("What This Does, In Plain Terms", "explanation",
 "Five-minute orientation for someone who has heard the word 'stingray' and nothing more. "
 "No protocol vocabulary anywhere on this page. Ends by routing to Is This For Me and Quick Start."),

"threat-models.md": ("Is This For Me?", "explanation",
 "Help the reader decide whether this tool addresses their actual risk. Cover the realistic "
 "user groups (journalists, organizers, researchers, the merely curious) and be honest that "
 "for many people the answer is 'this is interesting but not protective'. Link EFF's protest "
 "surveillance guide. Do not sell."),

"quick-start.md": ("Quick Start: From Box to First Recording", "tutorial",
 "Single device, single path, guaranteed outcome. Expected result after every step. Inline "
 "error recovery. See DOCS_PROMPT.md Phase 3."),

"first-warning.md": ("Your First Warning", "tutorial",
 "Walk through a warning end to end using the demo feature so the reader can rehearse before "
 "it happens for real. Routes interpretation to concepts/interpreting-warnings.md."),

"concepts/cell-networks.md": ("How Cell Networks Work", "explanation",
 "Pre-training page: establishes the vocabulary every later page depends on. Base station, "
 "cell, attach, identity (IMSI/TMSI/IMEI), paging, broadcast messages, handover, generations. "
 "Open with a concrete situation, not a definition. Source: EFF white paper section 2."),

"concepts/cell-site-simulators.md": ("What a Cell-Site Simulator Is", "explanation",
 "What the device is, who makes and buys them, what they cost, why phones fall for them. "
 "Dabrowski's identification vs camping modes. Sources: EFF SLS page, EFF white paper, "
 "Dabrowski 2014."),

"concepts/attacks.md": ("How These Attacks Actually Work", "explanation",
 "Hub page. The four attack families in one paragraph each, then link out. Keep it short; "
 "the depth lives in the children."),

"concepts/attack-identity.md": ("Capturing Your Identity", "explanation",
 "Permanent vs temporary identifiers, why the permanent one matters, the identify-then-reject "
 "pattern. Maps to the imsi_requested detector. Sources: EFF white paper, Dabrowski 2014 "
 "identification mode."),

"concepts/attack-downgrade.md": ("Downgrading You to Weaker Networks", "explanation",
 "Why an attacker wants you on 2G, how redirection and reselection priorities are abused, "
 "and the geographic caveat that 2G is normal in many countries. Maps to two detectors. "
 "Sources: EFF 2023 Apple/Google post, Shaik 2016."),

"concepts/attack-encryption.md": ("Turning Off Encryption", "explanation",
 "Cipher negotiation, the null algorithms and why they exist in the spec at all, why a fake "
 "tower needs them. Maps to null_cipher and nas_null_cipher. Sources: TS 33.401, EFF white "
 "paper section on dealing with encryption."),

"concepts/attack-location.md": ("Making Your Phone Report Its Position", "explanation",
 "The network can ask the phone to measure and report its own position. Positioning methods "
 "(A-GNSS, OTDOA, E-CID), one-shot vs periodic reporting. Maps to the LPP and RRLP detectors, "
 "which are this fork's addition. Sources: TS 36.355, TS 44.031, Shaik 2016 location leaks, "
 "EFF white paper on measurement reports."),

"concepts/why-detection-is-hard.md": ("Why Detection Is Hard", "explanation",
 "A passive observer sees only what the network sends this device. No ground truth, no "
 "labeled data, attacker adapts, benign networks are misconfigured all the time. Sources: "
 "White-Stingray, SeaGlass."),

"concepts/interpreting-warnings.md": ("Reading Warnings Without Panicking", "explanation",
 "The trust-calibration page. Base rates with a worked numerical example, what raises and "
 "lowers confidence, what to do at each severity, how to describe a finding defensibly. "
 "See DOCS_PROMPT.md Phase 1b. This page gets the most review."),

"concepts/limitations.md": ("What This Tool Cannot Tell You", "explanation",
 "Explicit list. Cannot identify who operates a device, cannot see traffic to other phones, "
 "cannot detect fully passive interception, cannot prove targeting, coverage gaps by "
 "generation and band. Written plainly, not as a disclaimer."),

"concepts/risk.md": ("Legal and Personal Risk", "explanation",
 "Running the tool, carrying the device, publishing findings. Jurisdiction-dependent; do not "
 "give legal advice, give the shape of the questions and point to counsel. Adapt upstream's "
 "disclaimer position."),

"devices/index.md": ("Device Notes", "reference",
 "Landing page for the per-device pages. Comparison table: device, availability, what works, "
 "known problems, difficulty."),

"web-interface.md": ("The Web Interface, Panel by Panel", "how-to",
 "Every panel in the UI and what it means. This fork adds several (cell site panel, identity "
 "panel, system information, packet explorer). Check against the Svelte components."),

"device-display.md": ("The Device Screen", "how-to",
 "Display modes, colours, status line height, keep-screen-on, button behaviour, the "
 "twenty-second suppression. All fork additions; see UPSTREAM.md display-colours, "
 "status-line-height, keep-screen-on, button-pauses-the-overlay."),

"recordings.md": ("Recordings: Naming, Notes, and Rotation", "how-to",
 "Display names, notes, rotation by size or time, auto-deletion of clean recordings and how "
 "naming protects one from it. Fork features; see UPSTREAM.md."),

"packet-explorer.md": ("The Packet Explorer", "how-to",
 "Browsing messages in a recording, filters, severity badges, jump to packet. Fork feature. "
 "Note it carries a report format version bump."),

"web-authentication.md": ("Securing the Web Interface", "how-to",
 "Optional accounts, off by default. State plainly that there is no TLS on these devices, so "
 "this is a second factor beyond the WiFi password and not a secure channel. UPSTREAM.md "
 "says the interface itself says so; keep the doc consistent with it."),

"sharing-findings.md": ("Sharing What You Find", "how-to",
 "Exporting a recording, what identifiers it contains and how to redact them, where to report, "
 "how to describe a finding without overstating. Pairs with interpreting-warnings.md."),

"severity.md": ("Severity, and What It Means", "explanation",
 "The severity levels, what each is meant to convey, why informational-only rows are not "
 "written (AnalysisRow::is_empty), and what that implies for reading a report."),

"detectors/index.md": ("Detector Reference", "reference",
 "Table of every detector: plain name, code identifier, severity, default on/off, validation "
 "status (real traffic / encoder vectors / synthetic only). The validation column is the "
 "point of this page."),

"detectors/imsi-requested.md": ("Identity Requested Without Authentication", "reference",
 "STYLE.md section 6 template. Source: lib/src/analysis/imsi_requested.rs. Known false "
 "positive: aircraft on approach."),
"detectors/connection-redirect-downgrade.md": ("Redirected to 2G", "reference",
 "Template. Source: lib/src/analysis/connection_redirect_downgrade.rs."),
"detectors/priority-2g-downgrade.md": ("2G/3G Advertised Above 4G", "reference",
 "Template. Source: lib/src/analysis/priority_2g_downgrade.rs. Note the history of false "
 "alarms in earlier versions and what changed."),
"detectors/null-cipher.md": ("Encryption Disabled (RRC)", "reference",
 "Template. Source: lib/src/analysis/null_cipher.rs."),
"detectors/nas-null-cipher.md": ("Encryption Disabled (NAS)", "reference",
 "Template. Source: lib/src/analysis/nas_null_cipher.rs. Explain how this differs from the "
 "RRC one and why both exist."),
"detectors/incomplete-sib.md": ("Incomplete System Information", "reference",
 "Template. Source: lib/src/analysis/incomplete_sib.rs."),
"detectors/lpp.md": ("Location Requested (LPP)", "reference",
 "Template. Source: lib/src/analysis/lpp.rs. Fork addition, two separately toggleable "
 "analyzers (basic and deep). MUST state: verified against pycrate's reference 36.355 "
 "encoder, caught a real one-bit layout error, not seen against a real capture. Note the "
 "Low-severity choice and why (informational-only rows are never written)."),
"detectors/rrlp.md": ("Location Requested on 2G (RRLP)", "reference",
 "Template. Source: lib/src/analysis/rrlp.rs. Fork addition. MUST state: verified against "
 "pycrate TS 44.018 and 44.031 encoders, never seen against real 2G traffic because the test "
 "devices are LTE. Note that detection needs both a valid APPLICATION INFORMATION header and "
 "a decodable RRLP APDU, so a wrong message-type guess cannot alone produce a false positive."),

"detectors/writing-a-detector.md": ("Writing a New Detector", "how-to",
 "For contributors. The Analyzer trait, where to hook in, config wiring, the heuristics.ts "
 "entry, the demo scenario, and the test vector expectation."),
"detectors/validation.md": ("How We Validate Detectors", "explanation",
 "The standard of evidence: reference-encoder vectors, demo round trip, real captures. Be "
 "explicit that several detectors have only the first two. This page is what makes the "
 "honesty in the detector pages legible as a policy rather than an admission."),

"fork/index.md": ("Why This Fork Exists", "explanation", "See DOCS_PROMPT.md Phase 4."),
"fork/features.md": ("What It Adds", "reference", "See DOCS_PROMPT.md Phase 4. Group by user goal."),
"fork/differences.md": ("Differences You Will Notice", "explanation", "See DOCS_PROMPT.md Phase 4."),
"fork/compatibility.md": ("Compatibility With Upstream", "reference",
 "Config, recording, and report format compatibility in both directions. Note the report "
 "format version bump carried by the packet explorer."),
"fork/upstreaming.md": ("Contributing Upstream", "how-to",
 "User-facing summary of UPSTREAM.md's process. Do not duplicate the whole file; link it."),

"configuration-reference.md": ("Configuration Reference", "reference",
 "Every key: name, type, default, effect, what breaks if wrong. Generated from Phase 0 "
 "inventory. configuration.md stays the how-to; this is the exhaustive table."),
"report-format.md": ("Report Format", "reference",
 "The analysis report JSON, by version. Note version 3 and what the packet explorer added."),
"glossary.md": ("Glossary", "reference",
 "Every term used anywhere in the book, defined in one or two plain sentences. Plain phrase "
 "first, acronym second. Link from first use on each page."),
"troubleshooting.md": ("Troubleshooting", "how-to",
 "Symptom, cause, fix. Include verbatim error strings so search finds them. Mine the "
 "not-this-device and gps-page-load fixes in UPSTREAM.md for real cases."),
"disclaimer.md": ("Legal Disclaimer", "reference",
 "Adapt upstream's. Keep an explicit statement about legal risk and jurisdiction."),
}

DETECTOR_TEMPLATE = """
## What you would see

## Why it matters

## When it fires harmlessly

## How it works

## Precise behavior

## Sources
"""

created, skipped = [], []
for rel, (title, mode, brief) in PAGES.items():
    path = os.path.join("doc", rel)
    if os.path.exists(path):
        skipped.append(rel)
        continue
    body = f"""# {title}

<!--
MODE: {mode}
BRIEF: {brief}

Binding: STYLE.md. Cite from doc/references.md. Mark unknowns as NEEDS INPUT.
-->
"""
    if rel.startswith("detectors/") and rel not in (
        "detectors/index.md", "detectors/writing-a-detector.md", "detectors/validation.md"):
        body += DETECTOR_TEMPLATE
    if DRY:
        print("would create", path)
    else:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as fh:
            fh.write(body)
    created.append(rel)

print(f"\n{'would create' if DRY else 'created'}: {len(created)}")
if skipped:
    print(f"skipped (already exist): {len(skipped)}")
    for s in skipped:
        print("   ", s)
print("\nNext: replace doc/SUMMARY.md with the new one, then run 'mdbook build' to verify.")
