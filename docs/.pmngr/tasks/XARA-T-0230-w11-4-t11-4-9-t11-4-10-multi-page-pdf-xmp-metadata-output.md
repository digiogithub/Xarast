---
id: XARA-T-0230
type: task
title: "W11.4 T11.4.9/T11.4.10: multi-page PDF, XMP metadata, output intent; qpdf --check in CI"
status: backlog
priority: low
parent: XARA-US-0059
author: mcp
labels: [phase-11, io, pdf, ci]
created: 2026-09-23T21:35:10Z
updated: 2026-09-23T21:35:10Z
---

## Description
Round 1 writes one page per export (MediaBox = area, TrimBox/BleedBox when there is a bleed, an Info dictionary with Producer only, no dates, DeviceRGB). Remaining:
- one PDF page per document page (`Capabilities::multipage`), page selection;
- XMP metadata (deterministic: no timestamps unless asked), an sRGB ICC output intent (`embed_output_intent`), document title;
- T11.4.10: `qpdf --check` over every exported PDF in CI (qpdf is not installed on the dev machine; the local gates used Poppler `pdftoppm` and Ghostscript, both clean on all 59 corpus files), and the Poppler render comparison (`crates/xarast-io/tests/pdf.rs`, skips when `pdftoppm` is absent) enabled in a CI job.

## Acceptance Criteria
- Criterion 8's structural clauses: one page per exported page, `qpdf --check` without warnings.
- The render comparison runs in CI, not only where `pdftoppm` happens to be installed.
