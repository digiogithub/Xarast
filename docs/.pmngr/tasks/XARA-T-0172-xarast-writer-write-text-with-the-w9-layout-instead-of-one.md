---
id: XARA-T-0172
type: task
title: ".xarast writer: write text with the W9 layout instead of one approximate <tspan> per line"
status: in_review
priority: high
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, format]
created: 2026-09-23T18:36:06Z
updated: 2026-09-23T20:31:21Z
started: 2026-09-23T18:56:25Z
---

## Description

The `.xarast` writer (`crates/xarast-format/src/svg/emit.rs`, `text`/`text_line`) still writes a story approximately: one `<tspan>` per `TextLine` at `x = 0`, lines spaced by `size × 1.2 × ratio`, runs split on font/fill. Since the text-render round (XARA-US-0045, walker text path) the application draws text from the real layout, so a `.xar` → `.xarast` → reopen round trip now changes every text file's pixels: the 19 corpus files with text are listed in `KNOWN_TEXT_GAPS` in `crates/xarast-app/tests/xarast_roundtrip.rs` (all 14 `TextDesigns`, `Designs/GardenPlan.xar`, `Designs/TextCurve.xar`, `testfiles/ProbeX16.xar`, `ScaleTest.xar`, `ScaleTest2.xar`).

What W9 now makes available to the writer (owner: the phase-6 writer agent; nothing here changes `xarast-format` yet):

- `xarast_doc::StoryText::collect` — the story's logical text, attribute runs (`CharRun { range, attrs: ResolvedAttrs }`), lines with their line-level attributes, manual kerns at byte offsets. The writer can emit runs from it instead of re-resolving attributes itself.
- `xarast-app`'s layout path (`text::build_story`, `Shaper::layout`) gives exact line baselines, x positions per glyph (millipoints) and substitutions; the writer could emit `x`/`y` per `<tspan>` (or per glyph with `x="…"` lists) so browsers/Inkscape place lines where Xarast does, while `xarast:` attributes keep the model (tracking em/1000, kerns em/1000 of 'M', baseline shift, script, aspect, justification, line spacing ratio/absolute, margins, ruler).
- Attributes are now scoped per string on import (`xar:` commit "scope text record attributes"): each string's attributes come before its characters and the importer inserts "restore" attributes after them. The writer's sibling-scoped reading is correct for that; the round trip must keep the restores.
- Kerns (`TextItem::Kern`) and tracking are currently dropped by the writer.
- The original's default text size is 16 pt (`Kernel/txtattr.cpp:449-452`) but the model's `default_for(TxtFontSize)` stays 12 pt, because the writer/reader elide defaults with their own 12 000 mp fallbacks (`emit.rs:1820`, `read/normal.rs:1015`, `read/build/ink.rs:728`); changing the model default broke the normal-form round trip. The importer writes 16 pt explicitly on stories that rely on it. Aligning the writer/reader fallbacks with `default_for` would let the model default become 16 pt.

## Acceptance Criteria

- The 19 files leave `KNOWN_TEXT_GAPS` and round-trip within the test's stated tolerance.
- Tracking, manual kerns, baseline shift, script and aspect survive a round trip (model digest equal on text nodes).
- A browser still shows readable, correctly placed text (graceful degradation).

## Notes

Filed by the text-integration agent; see `docs/memory/text.md` "Walker integration".
