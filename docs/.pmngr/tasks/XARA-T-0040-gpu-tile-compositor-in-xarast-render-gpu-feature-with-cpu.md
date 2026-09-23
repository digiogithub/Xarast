---
id: XARA-T-0040
type: task
title: GPU tile compositor in xarast-render (gpu feature) with CPU parity test and bench
status: in_progress
parent: XARA-US-0011
author: mcp
labels: [render, gpu]
created: 2026-09-23T12:04:02Z
updated: 2026-09-23T12:04:02Z
started: 2026-09-23T12:04:02Z
---

## Description
The smallest step the decision earns: a GPU-resident tile cache of CPU-rasterised pixels that composites onto a Rgba8Unorm target under an axis-aligned view change (pan, Draft zoom resample), with a CPU reference implementation of the same mapping for the software tier.

## Acceptance Criteria
- Parity test against the CPU reference on real hardware (skips without an adapter).
- Bench on both GPUs: dirty-tile upload + composite vs today's full-frame upload.
- clippy/tests clean with and without the `gpu` feature.
