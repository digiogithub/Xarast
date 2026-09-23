---
id: XARA-T-0160
type: task
title: T4.4 Rotate about the rotation centre, Ctrl constrains to 45° steps
status: done
parent: XARA-US-0032
author: mcp
labels: [phase-7, tools]
created: 2026-09-23T18:10:41Z
updated: 2026-09-23T18:10:41Z
---

## Notes
Commit 69d674e. Test `a_corner_drag_in_rotate_mode_rotates_about_the_centre` (one "Rotate" step, exact undo/redo, Angle field 90°). Live: `--probe rotate` on GardenPlan.xar, p50 1.8 ms input→present.
