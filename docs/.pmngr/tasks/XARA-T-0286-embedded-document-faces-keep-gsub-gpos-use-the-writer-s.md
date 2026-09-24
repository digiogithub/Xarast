---
id: XARA-T-0286
type: task
title: "Embedded document faces: keep GSUB/GPOS, use the writer's substitute, @font-face style, browser probe"
status: backlog
priority: low
parent: XARA-US-0049
author: mcp
labels: [phase-9, text, format]
created: 2026-09-24T10:12:38Z
updated: 2026-09-24T10:12:38Z
---

## Description

Gaps found while building XARA-T-0276 (embedded fonts on read, WOFF2 glyf transform; see `docs/memory/text.md`, "Embedded fonts on read").

1. **No GSUB/GPOS in a web font.** `subsetter` drops layout tables and renumbers glyphs, so a document face is laid out by Xarast *without* kerning, ligatures or Arabic joining when the machine lacks the face (and browsers draw it the same way). Subsetting GSUB/GPOS (or keeping the whole face when small) would make the reader's layout the writer's.
2. **Substituted families.** A run asking for "Arial" drawn with "Liberation Sans" embeds "Liberation Sans"; a reader without either uses its own ladder, and the embedded face is used only if the ladder reaches the same family. The run's `xarast:font-substitute` twin could point the reader at the embedded face.
3. **`@font-face` style.** It declares the face's own weight/style, so a browser may synthesise bold where Xarast does not (the `research/06` rule 3 polish left from XARA-T-0276).
4. **Browser probe on transformed files.** Our WOFF2 now uses the glyf transform; the Chrome probe of XARA-T-0218 was run on null-transform files only. The decoder is checked against a `ttf2woff2` fixture, the encoder only against our decoder.

## Acceptance Criteria

- A document opened without its face lays out with the same kerning as with it (a text with kerning pairs, pixel-identical).
- A substituted run opens with the writer's substitute face where the reader has neither.
- Chrome (headless) renders a transformed-WOFF2 SVG export with the embedded glyphs.
