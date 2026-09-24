---
id: XARA-T-0303
type: task
title: Photo operations on bitmap fills and bitmap transparencies
status: backlog
parent: XARA-US-0054
author: mcp
labels: [phase-10, image]
created: 2026-09-24T13:19:22Z
updated: 2026-09-24T13:19:22Z
---

## Description
XARA-US-0054 attaches a `PhotoOps` chain to placed bitmap objects (`BitmapNode::photo_ops`) only, as phase 10 §"Non-destructive photo pipeline" describes. A bitmap fill (`FillGeometry::Bitmap`) or a bitmap transparency cannot carry one. Decide whether they should (the original's XPE edits applied to the bitmap, so every use changed), and if so where the chain lives (the fill attribute, so the walker's derived-image cache keys on it too) and how `.xarast` writes it on the `<pattern>` image and the transparency twin.

## Acceptance Criteria
- A decision recorded in `docs/memory/image.md`; if implemented, a fill with a chain renders, round-trips and exports like a placed bitmap's.
