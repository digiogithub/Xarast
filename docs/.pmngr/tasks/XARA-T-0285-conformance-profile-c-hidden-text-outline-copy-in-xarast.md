---
id: XARA-T-0285
type: task
title: "Conformance profile C: hidden text-outline copy in .xarast, ignored by the reader"
status: backlog
priority: low
parent: XARA-US-0049
author: mcp
labels: [phase-9, text, format]
created: 2026-09-24T10:12:38Z
updated: 2026-09-24T10:12:38Z
---

## Description

Split out of XARA-T-0276 (part 2, not built there). `research/06 §6.7` rule 5 and §14.2: an archival (profile C) save must carry, per story, a copy of the text as curves in `<g xarast:generated="text-outline" style="display:none">`, which a reader may enable when the font is unavailable.

- There is no profile C writer: `Profile` has only `Portable`/`Compact`, and `SvgOptions` has no archival switch.
- The geometry exists: `xarast_app::text::story_outlines` (one outline per attribute run, with its attributes) and `convert::text_as_outlines`. The writer would need it through the `TextPlacer` seam (e.g. `fn outlines(&self, doc, story, attrs)`), and the paint of each run.
- **Reader risk:** today a `<g xarast:generated=...>` whose generator is not present becomes an ordinary group of editable geometry (orphan rule). The reader must *drop* a `text-outline` group whose story is present, or the copy becomes duplicate art on the first round trip.

## Acceptance Criteria

- A profile C save carries one hidden `text-outline` group per story with its runs' outlines.
- The reader ignores it (normal form and render unchanged, 59/59 round trip), and keeps it out of the model.
- A browser shows nothing extra (display:none).
