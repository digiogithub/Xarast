---
id: XARA-T-0262
type: task
title: TextDesigns goldens against the original's reference bitmaps (metric-compatible pinned faces)
status: backlog
priority: medium
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, render]
created: 2026-09-24T01:27:28Z
updated: 2026-09-24T01:27:28Z
---

## Description

Filed from XARA-US-0050. `xarast-cli/tests/text_designs.rs` pins each TextDesigns render to an exact SHA-256 (gate A: a regression gate). With the pinned set, Arial, Times New Roman, Calisto MT, Book Antiqua and the embeddedFonts faces all fall to Noto Sans, which is wider than Arial, so the goldens do not show the layout agreeing with the reference bitmap of the original's rendering that every TextDesigns file carries. On the host's fonts (Liberation Sans for Arial) nine files overlay within about a pixel (text.md, "The acceptance fixture, round 2"); no test holds that.

## Acceptance Criteria

- Vendor OFL-licensed metric-compatible faces (Liberation Sans / Serif 2.x subsets, or equivalent) in a separate pinned directory, so the existing `Noto Sans` substitution assertions and shaping goldens do not move.
- A test renders each TextDesigns file twice (with and without the story layer, or by hiding the reference bitmap) and measures per-glyph displacement or the perceptual gate C against the carried reference bitmap, with numbers recorded in `docs/memory/text.md`.
- No corpus bytes and no renders committed; only derived metrics.
