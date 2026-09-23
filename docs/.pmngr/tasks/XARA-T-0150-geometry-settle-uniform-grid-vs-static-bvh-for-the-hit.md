---
id: XARA-T-0150
type: task
title: "Geometry: settle uniform grid vs static BVH for the hit index at 100k objects (benchmark verdict)"
status: done
parent: XARA-US-0031
author: mcp
labels: [phase-7, geometry, perf]
created: 2026-09-23T17:39:33Z
updated: 2026-09-23T17:39:33Z
---

## Description

Criterion benchmark `crates/xarast-geom/benches/hit_index.rs` (reference machine, `taskset -c 4-7`): the shipped grid against a median-split BVH with max-z best-first picking, subtree slices for enclosed nodes and O(log n) refit, over four 100k-object scenes (uniform, clustered, mixed sizes, all stacked on one point), plus the whole precise pick.

## Verdict: the uniform (hashed) grid

The BVH answers queries faster (pick 0.56–6.3 µs vs 2.2–9.9 µs; full-page marquee 5–37 µs vs 0.78–1.38 ms), but both are two to three orders of magnitude inside the budgets, while insert/remove costs the grid 23–30 ns and a static BVH a full 8–23 ms rebuild — more than a frame on every create, delete, paste and undo.

## Budgets (phase-07: pick ≤ 2 ms, marquee ≤ 20 ms; assignment: 1 ms / 5 ms)

- Topmost precise pick among 100k: 5.5–13.8 µs — passes both.
- Marquee over 100k: worst 1.38 ms (whole page) — passes both.
- **Adversarial hollow stack** (100k unfilled outlines all around the point, every candidate needs a precise test): 17.7–21 ms — fails; intrinsic to any index (≈180 ns per precise rejection). Recorded as a risk in `docs/memory/geometry.md`.

Commits: 06c885f (+ the docs commit).
