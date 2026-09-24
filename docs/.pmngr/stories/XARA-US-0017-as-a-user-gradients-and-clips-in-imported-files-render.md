---
id: XARA-US-0017
type: story
title: As a user, gradients and clips in imported files render faithfully
status: done
parent: XARA-EP-0018
author: mcp
labels: [render, app-core]
estimate: 5
created: 2026-09-23T09:40:52Z
updated: 2026-09-24T16:07:18Z
started: 2026-09-24T14:44:31Z
closed: 2026-09-24T16:07:18Z
---

## Acceptance Criteria
- `RampMapping::Sin` honoured (bake easing into stops or add to `build_ramp`).
- Perspective gradient `p2`/`p3` ordering confirmed against a golden image.
- Decide when `Tiling::Repeat` becomes `Repeat::RepeatHq`.
- `ClipViewMode::Outside` supported (inverted path or mask layer) instead of dropped.
