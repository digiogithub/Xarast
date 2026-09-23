---
id: XARA-T-0173
type: task
title: Text renders in the scene walker (every corpus story drawn)
status: done
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, app]
created: 2026-09-23T18:48:21Z
updated: 2026-09-23T18:48:35Z
---

## Description

Lay out each story with `xarast-text` and draw its glyph outlines in the walker, with the text's resolved fills, strokes and transparency; one shared font service per process, system fonts enumerated off the UI thread; substitutions reported.

## Acceptance Criteria

- `text_pending` 0 on the corpus; only `Designs/TextCurve.xar` in `text_on_path_pending`.
- All 14 `TextDesigns` draw glyphs besides their reference bitmap.
- Tests never see system fonts.

## Notes

Done on branch `worktree-agent-a8925f0d919c9d2c8`: commits 42a7e28 (xar scoping + surrogates), 19d49d9 (StoryText), b13af5a ('M' em width, PANOSE), 53899b6 (walker), 6fe3050 (shell prewarm + status), 9acfb5b (16 pt story default), da059d4 (docs). Tests: `crates/xarast-app/tests/text.rs`, `corpus.rs::every_corpus_story_is_drawn`, `a_missing_font_is_substituted_and_reported`.
