---
id: XARA-T-0233
type: task
title: "T11.3.4 — SVG export: embed fonts (@font-face WOFF2 subset) or write text as outlines"
status: in_review
parent: XARA-US-0058
author: mcp
labels: [phase-11, io, svg, text]
created: 2026-09-23T22:10:40Z
updated: 2026-09-24T08:51:14Z
started: 2026-09-24T08:50:57Z
---

## Description
SVG export (XARA-US-0058) writes text as live `<text>` in the fonts the document names, placed per character by the application's `TextPlacer`. Every family is reported as `Compromise::FontNotEmbedded`; a viewer without the font substitutes (corpus: Arial, Times New Roman, Courier New, Calisto MT, Book Antiqua, Myriad Web, Margaret, Calligraphic, KirbysHand).

Add the phase's `TextOutput` option: `AsText { embed_fonts }` inlining a WOFF2 subset as a `data:` URI in `@font-face` (same subset code and `fsType` check as `.xarast` font embedding, phase 9 W9.6), and `AsOutlines`, which replaces text with the glyph outlines the scene walker already produces (the PDF exporter draws them). Outlines need either a `TextPlacer` extension returning outlines or the exporter reading them from `SceneWalker` — decide on the seam with the app owner (`xarast-app::svg_text`).

## Acceptance Criteria
- `--text outlines` gives an SVG with no `<text>` and no `FontNotEmbedded` entries; resvg render within the current mean |Δ| of the PNG export on the TextDesigns files.
- Embedding respects `fsType`; a refused font falls back to outlines and is reported.

## Notes
Seam today: `ExportSource::svg_text_placer`, `xarast_format::svg::TextPlacer`.
