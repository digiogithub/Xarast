---
id: XARA-T-0016
type: task
title: Write fuzz_path_boolean (xarast-geom)
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, geometry]
created: 2026-09-23T10:23:29Z
updated: 2026-09-23T10:23:29Z
---

## Description
`arbitrary` -> two paths via the public builder (coordinates mostly snapped to a coarse grid to force coincidence) -> a fuzzer-chosen `BoolOp` x `FillRule` -> `boolean` and `self_union` output must pass `Path::validate()`; `A - A` and `A ∩ ∅` must be empty.

## Notes
One op/rule pair per input rather than all 16: an extent-sized cubic flattens to ~9 000 vertices at `Tolerance::BOOLEAN` (~35 ms per overlay in release), so 16 per case timed out under instrumentation.
