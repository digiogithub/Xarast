---
id: XARA-T-0164
type: task
title: T5.2 Interactive corner-radius handle on the rectangle
status: done
parent: XARA-US-0033
author: mcp
labels: [phase-7, tools]
created: 2026-09-23T18:10:55Z
updated: 2026-09-23T18:10:55Z
---

## Notes
Commit 69d674e. Handle on the first side, projected and clamped to half the shorter side; one "Edit Shape" step via SetShapeParams; node stays a QuickShape. Test `the_radius_handle_rounds_the_corners_and_the_infobar_edits_the_shape`.
