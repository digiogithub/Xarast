---
id: XARA-T-0032
type: task
title: Vertical ruler labels are clipped by the 18 pt ruler strip
status: backlog
priority: low
parent: XARA-US-0081
author: mcp
labels: [ui]
created: 2026-09-23T10:59:46Z
updated: 2026-09-23T10:59:46Z
---

## Description
In the real window (BLUECAR.xar, 1.25×), the vertical ruler draws labels such as `1450pt` horizontally inside an 18 pt wide strip (`RULER_THICKNESS`). Only the first 3–4 digits show, and the unit suffix is cut off. The bug is older than XARA-T-0026, which only made the values positive.

## Acceptance Criteria
- Vertical ruler labels are fully legible at 1×, 1.25× and 2×. Options: draw them rotated 90°, stack the digits, or drop the unit suffix on the vertical ruler.
- A test asserts that the label's laid-out extent fits the strip.

## Notes
Found while verifying XARA-T-0026 with `xarast --screenshot`.
