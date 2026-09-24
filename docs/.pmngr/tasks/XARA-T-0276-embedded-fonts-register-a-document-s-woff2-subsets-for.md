---
id: XARA-T-0276
type: task
title: "Embedded fonts: register a document's WOFF2 subsets for display, and the profile C text-outline copy"
status: in_progress
priority: low
parent: XARA-US-0049
author: mcp
labels: [phase-9, text, format]
created: 2026-09-24T08:50:25Z
updated: 2026-09-24T09:47:49Z
started: 2026-09-24T09:47:49Z
---

## Description

Leftovers of XARA-T-0218 / XARA-T-0245 (font embedding and the export outline fallback are done; see `docs/memory/text.md`, "Font embedding").

1. **Reading embedded fonts.** `.xarast` now carries a WOFF2 subset per face in `resources/fonts/`, but the reader does not use them: a machine without the face substitutes, as before. Using them needs a *per-document* font overlay (today `FontDb::register_embedded` is process-wide, so a document's subset would shadow the family for every document) and must keep the subset for display only — typing a character the subset lacks must fall back to a real face, never draw `.notdef`. Needs a WOFF2 decoder that accepts other writers' files (ours only uses null transforms; `woff2::decode` refuses the glyf transform).
2. **Conformance profile C** (`research/06 §6.7` rule 5, §14.2): archival mode writes a copy of the text as curves in `<g xarast:generated="text-outline" style="display:none">`. There is no profile C writer yet; the geometry exists (`xarast_app::convert::text_as_outlines`, `text::story_outlines`).
3. Web-font polish: the WOFF2 `glyf` transform (a few per cent smaller files), and `@font-face` weight/style declaring the requested style so a browser does not synthesise bold where Xarast does not.

## Acceptance Criteria

- A `.xarast` whose face is not installed renders with its embedded subset in Xarast, without that subset leaking into other open documents, and typing outside the subset uses a real face.
- A profile C save carries the hidden outline copy; the reader ignores it.
