---
id: XARA-T-0100
type: task
title: "F3.11 — Security: never emit script, on*, foreignObject, SMIL, external references, DTDs"
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:30Z
updated: 2026-09-23T15:40:30Z
---

## Description
The writer produces none of them; every href is `#id` or `resources/…`; all text and attribute values are escaped and XML-forbidden characters dropped; foreign baggage is re-emitted only as a well-formed fragment (one balanced element, a comment or a PI; no DOCTYPE/CDATA at top level), attribute names checked as NCNames. Asserted over the fixture and over all 59 corpus files (`tests/svg.rs`, `tests/svg_corpus.rs`).

## Notes
Done in eb285b2. Stripping active content *inside* captured baggage is the reader's job (W4, §5.3).
