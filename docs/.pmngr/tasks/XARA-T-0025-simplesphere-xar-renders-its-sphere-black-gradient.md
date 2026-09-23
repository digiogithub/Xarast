---
id: XARA-T-0025
type: task
title: SimpleSphere.xar renders its sphere black (gradient + transparency stack)
status: backlog
priority: medium
parent: XARA-US-0081
author: mcp
labels: [render, app-core]
created: 2026-09-23T10:31:45Z
updated: 2026-09-23T10:36:14Z
---

## Description
`Designs/SimpleSphere.xar` is about 2000 large shapes with graduated colour and graduated transparency. It renders as a black rectangle with only the caption text visible. The result is identical in the running window and in `cargo run -p xarast-app --example render_headless`, so this is a walker/renderer fidelity gap, not a composition bug. It was found while verifying XARA-US-0001.

## Acceptance Criteria
- Find which stage loses the sphere: transparency ramp mapping, gradient mapping, or the blend family.
- The sphere is visible in a headless render, and a golden or pixel-probe test pins it.
