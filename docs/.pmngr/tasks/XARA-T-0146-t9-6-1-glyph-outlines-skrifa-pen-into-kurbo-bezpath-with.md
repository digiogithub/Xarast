---
id: XARA-T-0146
type: task
title: "T9.6.1 — Glyph outlines: skrifa pen into kurbo::BezPath with variations"
status: done
parent: XARA-US-0049
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:42Z
updated: 2026-09-23T17:24:42Z
---

## Description
`FontDb::glyph_outline(face, glyph, &[FontVariation])` and `glyph_outline_normalized(face, glyph, coords)`: unhinted, font units, y up. `GlyphRun::glyph_transform(g, upem)` maps to story space (size/upem, aspect).

## Acceptance Criteria
- Synthetic variable font: rectangle edge follows wght (100/400/650/900, clamped); TrueType quads stay quads; CFF comes out as cubics; placed transform exact (`tests/outline.rs`).

## Notes
Commit 8a96272 (tests in f7c05dd).
