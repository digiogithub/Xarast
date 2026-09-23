---
id: XARA-T-0163
type: task
title: "T5.1/T5.3 Rectangle and ellipse tools: drag to create quick shapes, Ctrl square/circle, Shift from centre, current attributes"
status: done
parent: XARA-US-0033
author: mcp
labels: [phase-7, tools]
created: 2026-09-23T18:10:55Z
updated: 2026-09-23T18:10:55Z
---

## Notes
Commits 89e808b (CreateShape), 69d674e (`xarast-app/src/shapes.rs`, `EditState::current`, `Intent::SetCurrentAttribute`). One "Create Rectangle"/"Create Ellipse" step, new shape selected. Tests in `tests/transforms.rs`: creation, square/circle, from-centre, current attributes, Esc mid-draw. Live: `--probe rect|ellipse`. Preview is an overlay outline (no walker phantom support yet).
