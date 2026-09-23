---
id: XARA-T-0142
type: task
title: "T9.3.1 — Shaper façade over parley: style ranges in, positioned glyph runs out"
status: done
parent: XARA-US-0046
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:42Z
updated: 2026-09-23T17:24:42Z
---

## Description
`Shaper::layout(&StoryInput)` → `Layout { lines: Vec<LaidLine { runs: Vec<GlyphRun>, clusters, baseline_y, … }>, bounds, substitutions }`. Shapes with parley/harfrust at a nominal 1000 units/em, converts each advance to millipoints once and accumulates in millipoints. Ligatures and base+marks fold into one cluster. Features and variations per run; auto-kern off via `kern=0`. Bidi levels and per-line reordering via unicode-bidi; explicit base direction forced into parley with LRM/RLM. Graphemes capped at 64 chars (parley 0.11.1 u8 overflow).

## Acceptance Criteria
- Golden glyph sequences for the pinned Latin, Hebrew, Arabic and CJK faces (`tests/shaping.rs`).
- RTL: leftmost glyph has the highest cluster index; mixed-level digits; no .notdef in a mixed-script line.
- Budgets: 1k glyphs 0.22 ms (≤ 8 ms), 10k characters 2.28 ms (≤ 60 ms).

## Notes
Commits f7c05dd, benches 4e243b2.
