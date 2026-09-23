---
id: XARA-T-0144
type: task
title: T9.3.3 — Line breaking for column text (ICU4X segmentation)
status: done
parent: XARA-US-0046
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:42Z
updated: 2026-09-23T17:24:42Z
---

## Description
UAX #14 opportunities from ICU4X `LineSegmenter` (dictionary mode with the default `complex-scripts` feature), greedy fitting with the original's rules: spaces hang, a character fits if its advance without tracking fits, at least one cluster per line, emergency break between clusters for an overlong word, break before tabs. First-line indent replaces the left margin. `StoryMode::Column { width, wrap }`.

## Acceptance Criteria
- Wrapping, overlong word, non-wrapping column, CJK case (break between ideographs, never before 。) in `tests/layout.rs`.

## Notes
Commit f7c05dd.
