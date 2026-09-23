---
id: XARA-T-0243
type: task
title: "T9.6.3 — ConvertTextToShapes: story → group of run outlines (Ctrl+Shift+C)"
status: done
parent: XARA-US-0049
author: mcp
labels: [phase-9, text]
created: 2026-09-23T22:35:23Z
updated: 2026-09-23T23:20:36Z
started: 2026-09-23T22:35:23Z
closed: 2026-09-23T23:20:36Z
---

## Description
`xarast_doc::convert_story_to_shapes` (tree edit) + `xarast_app::convert::ConvertCommand` (layout with the walker's exact geometry). Joins the phase-7 "Convert to editable shapes" command on Ctrl+Shift+C; a selected group converts everything convertible inside it.

## Acceptance Criteria
- One path per attribute run, non-text attributes that differ from the inherited state as own attributes, non-zero winding.
- Every TextDesigns story converts with zero differing pixels; outline box = ink box within 1 mp; undo exact.

## Notes
Branch worktree-agent-a71f19eae8f81f9ca, commits cb0b3d1, b36d751.
