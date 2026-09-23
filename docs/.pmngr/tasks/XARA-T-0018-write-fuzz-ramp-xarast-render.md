---
id: XARA-T-0018
type: task
title: Write fuzz_ramp (xarast-render)
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, render]
created: 2026-09-23T10:23:48Z
updated: 2026-09-23T10:23:48Z
---

## Description
Arbitrary colour/transparency stops (NaN, inf, duplicates, unsorted), bias/gain profiles (clamped and raw), effect space, ramp length, gradient shape, mapping (affine and perspective, collinear handles) and probe points. Asserts table lengths, profile range, interning idempotence and that the cache holds what `build_ramp` built; `grad_param`/`apply_repeat`/`eval_paint` must not panic. Closes render.md TODO 9 (half).
