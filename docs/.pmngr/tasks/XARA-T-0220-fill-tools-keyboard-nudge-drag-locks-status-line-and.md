---
id: XARA-T-0220
type: task
title: "Fill tools: keyboard nudge, drag locks, status line and outline handles"
status: done
parent: XARA-US-0039
author: mcp
labels: [phase-8, tools]
created: 2026-09-23T21:18:03Z
updated: 2026-09-24T19:19:25Z
started: 2026-09-24T17:19:39Z
closed: 2026-09-24T19:19:25Z
---

## Description
Follow-ups the fill and transparency tools (XARA-US-0039) left open, from phase-08 §W8.3/§W8.4:
- T8.3.5: nudge the selected fill handle or stop from the keyboard, with Ctrl/Shift step modifiers (one coalesced step per run).
- T8.3.6 (the rest): axis lock and aspect lock during a handle drag. Constrain already snaps to 15°.
- T8.4.6: status-line text and a cursor for each hover target.
- Handles for the outline (stroke) paint slot; today only the interior has them.
- The original's double click followed by a drag, which makes a conical fill.
- The Hue blend mode (`TranspMode` has no variant for it).

## Acceptance Criteria
- Each item has a test through `Session::apply`, like `crates/xarast-app/tests/fill_tool.rs`.
