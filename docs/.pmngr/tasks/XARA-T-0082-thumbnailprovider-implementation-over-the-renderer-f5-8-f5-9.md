---
id: XARA-T-0082
type: task
title: ThumbnailProvider implementation over the renderer (F5.8/F5.9)
status: done
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-app]
created: 2026-09-23T14:23:15Z
updated: 2026-09-23T19:53:37Z
closed: 2026-09-23T19:53:37Z
---

## Description
Implement `xarast_format::ThumbnailProvider` in xarast-app over the CPU renderer: first spread's page area, RGBA8, 256 px longer side, transparent with the page colour composited beneath; per-spread previews at 512 px (`--no-previews` to skip). PNG encoding with the workspace `png` crate.

## Acceptance Criteria
- Output passes `thumbnail::check_png`; thumbnail of a 20 MB document within the save budget.

## Notes
Left over from round 1 (format crate must not depend on the renderer, research/06 §13.1).
