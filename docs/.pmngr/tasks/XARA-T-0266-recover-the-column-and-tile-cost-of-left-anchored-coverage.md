---
id: XARA-T-0266
type: task
title: Recover the column and tile cost of left-anchored coverage
status: backlog
parent: XARA-US-0040
author: mcp
labels: [phase-8, render, perf]
created: 2026-09-24T02:18:01Z
updated: 2026-09-24T02:18:01Z
---

## Description
XARA-T-0221 made coverage independent of the draw area: a primitive is now rasterised from `max(bounds.x0, 0)` and its band's top (`render.md` invariant 15), so a repaint rectangle, column, column tile or pan strip gives exactly the pixels of a whole-frame render. The price is that a column or strip re-rasterises the part of a wide primitive to its left. Measured on the viewport bench (224k primitives, 1080p): zoomed `pan_draft` 10.4 → 11.4 ms, zoomed `final_after_idle` 91 → 96–99 ms; fit unchanged (`perf.md`, "An edit repaints its damage").

Snapping the origin to vello's 4 px tile grid was tried and is not exact (`aa_edge_45` moves). Candidates: cache a primitive's band coverage across the columns of one Final frame; or rasterise wide primitives once per band and share the result between column tiles.

## Acceptance Criteria
- `determinism::coverage_does_not_depend_on_the_draw_area` still passes.
- The zoomed Final and pan figures are back to their earlier values, or the reason they cannot be is recorded in `perf.md`.
