---
id: XARA-T-0228
type: task
title: "W11.4 T11.4.7 PDF text: embedded subset fonts with ToUnicode (or outlines on fsType denial)"
status: done
priority: medium
parent: XARA-US-0059
author: mcp
labels: [phase-11, io, pdf, text]
created: 2026-09-23T21:35:10Z
updated: 2026-09-24T09:32:20Z
started: 2026-09-24T07:43:40Z
closed: 2026-09-24T09:32:20Z
---

## Description
Round 1 draws text as whatever the scene holds: glyph outlines as filled paths. PDF text is not selectable or searchable. Emit text runs as embedded, subset fonts (Type 0 / CIDFontType2 or CFF) with a correct `ToUnicode` CMap; fall back to outlines with `Compromise::FontNotEmbedded` when the font's `fsType` forbids embedding (share phase 9's check). Needs the scene (or an `ExportSource` side channel) to carry glyph runs, not just outlines.

## Acceptance Criteria
- Phase 11 criterion 8's font clause: every font used is embedded and subset.
- Text copied from the PDF in a viewer matches the document text.
- `Capabilities::embeds_fonts` becomes true for PDF.

## Notes
`subsetter` (Typst) is the obvious crate; check licence and the `skrifa` version it pins.
