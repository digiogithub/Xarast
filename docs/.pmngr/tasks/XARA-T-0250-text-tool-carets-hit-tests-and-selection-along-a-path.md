---
id: XARA-T-0250
type: task
title: "Text tool: carets, hit tests and selection along a path"
status: in_review
parent: XARA-US-0048
author: mcp
labels: [phase-9, text]
created: 2026-09-23T23:43:49Z
updated: 2026-09-24T00:32:13Z
started: 2026-09-24T00:15:40Z
---

## Description
Since W9.5 the walker draws text on a path along its path, but the text tool's `StoryView`/`CaretMap` (`xarast-app/src/text_tool.rs`) still lays the story out as a point story on a straight baseline, so the caret, click-to-position and selection spans of a story on a path are drawn and hit-tested away from the text. Lay out with `text::lay_story` (the path's column mode) and map every caret stop, hit test and selection span through `PathFit::cluster_transform` (inverse for hits: nearest cluster by its fitted box).

## Acceptance Criteria
- On `Designs/TextCurve.xar`, the caret at every stop of every line sits at the fitted cluster edge (within 1 mp) and turns with it.
- Clicking a fitted glyph puts the caret at that glyph (round trip over every stop).
- Selection spans are drawn along the path, one quad per cluster.
