---
id: XARA-T-0314
type: task
title: B9 — Cache offscreen effect layers instead of recomputing them every frame
status: in_review
priority: medium
parent: XARA-US-0068
author: mcp
labels: [phase-13, live-effects, render, perf]
created: 2026-09-24T18:06:47Z
updated: 2026-09-24T20:11:17Z
started: 2026-09-24T19:27:15Z
---

## Description
XARA-US-0068 renders every live effect (today: feathers) per frame on the CPU backend: the wrapped commands into an offscreen region twice (colour and silhouette), then an erosion and a disc blur, on every frame and every damage repaint that reaches it. Measured on the corpus (release, 100 %, warm): Groucho2 50 → 129 ms, feathers.xar 5 → 26 ms.

Phase 13 B9: key offscreen results by (node content hash, quantised pixel width, quality, variant), LRU-by-cost, sharing the Phase 4 render-cache budget (`xarast-render/src/cache.rs`). The walker already produces per-node content hashes; the effect needs an id (the owner's scene id) on `SceneOp::PushEffect` to be cacheable.

## Acceptance Criteria
- A pan or a repaint that does not change an effect's content reuses its layer (counted, tested).
- Byte identity with the uncached path: the corpus determinism tests (draw area, band height, threads) and the damage property stay exact (render.md invariant 23).
- The cache shares the existing byte budget; `perf.md` records the Groucho2 frame before/after.

## Notes
Design and exactness rules: `docs/memory/render.md`, "Live effects: the offscreen pipeline".
