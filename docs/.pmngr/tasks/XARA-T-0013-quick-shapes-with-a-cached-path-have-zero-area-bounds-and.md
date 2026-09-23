---
id: XARA-T-0013
type: task
title: Quick shapes with a cached path have zero-area bounds and are culled from the render
status: backlog
priority: medium
parent: XARA-US-0081
author: mcp
labels: [doc, render, bug]
created: 2026-09-23T10:04:32Z
updated: 2026-09-23T10:36:14Z
---

## Description
Found by `xarast-cli render` over the corpus (XARA-T-0004). Three corpus files, `testfiles/RedStar.xar`, `testfiles/Test00.xar` and `testfiles/TestBitmapFill.xar`, each hold one `QuickShape` with a cached path. `smoke-open` reports **1 primitive and a complete walk** for each (so `shapes_pending == 0`). Yet each renders **zero ink pixels**:

- The bounds of their drawable content have zero width or height, so the CLI's drawing frame falls back to the page.
- At page framing, `TestBitmapFill` produces a display list of 0 commands, and `RedStar` and `Test00` produce 1 command that paints nothing.

The likely cause is the cached path being in shape-local coordinates (or degenerate). `xarast_doc::bounds` (`QuickShape(q) => q.path.bounds()`) and the walker (`geometry_of`) both use it without the shape's transform. So the bounds are point-like, and `DisplayList::build` culls the command against the dirty rect.

This accounts for the gap between 38 files with scene primitives and 35 files with painted pixels at 100 %.

## Acceptance Criteria
- These three files paint pixels in `xarast-cli render` at 100 %.
- A `QuickShape` has non-degenerate bounds, and the walker places its path the same way the bounds do.
