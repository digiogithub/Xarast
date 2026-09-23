---
id: XARA-T-0247
type: task
title: "Colour editor: edit the fill tool's selected stop and gradient end colours"
status: in_progress
parent: XARA-US-0041
author: mcp
labels: [phase-8, app, ui]
created: 2026-09-23T22:44:51Z
updated: 2026-09-23T23:50:28Z
started: 2026-09-23T23:50:28Z
---

## Description
The colour editor's selection target edits `StopTarget::From` of each selected object's fill (a flat fill's colour, a gradient's start). When the fill tool is active with a handle or stop selected, the editor should edit that stop (`To`, `Mid(i)`, `Corner(i)`) through the same `SetStopValue` live/commit path, as the original does.

## Acceptance Criteria
- With the fill tool's end blob selected, a drag in the editor changes only `to` (whole-`FillGeometry` diff) in one undo step.
- The editor's title says which stop it edits.

## Notes
Found while closing XARA-US-0041 (`docs/memory/colour.md` decision 20). The fill tool keeps its selected handle privately; it needs a read-only accessor for the session projection.
