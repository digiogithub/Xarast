---
id: XARA-T-0010
type: task
title: Measure cold start (≤ 400 ms) of the xarast binary on the reference machine
status: backlog
priority: medium
parent: XARA-US-0010
author: mcp
labels: [perf, hardware]
created: 2026-09-23T10:00:29Z
updated: 2026-09-23T10:00:29Z
---

## Description
Budget: cold start ≤ 400 ms to an interactive empty window (roadmap performance table). Needs the wired viewer; deferred from round 1 of XARA-US-0010.

## Acceptance Criteria
- Process spawn to first presented frame, both as a plain release binary and as the AppImage, with a dropped page cache (`echo 3 > /proc/sys/vm/drop_caches`) and warm; median of ≥ 5.
- Adapter selection and pipeline/shader compilation time reported separately (the `vello` renderer compiles its shaders at start-up; a `wgpu::PipelineCache` may be needed).
- Recorded in `docs/memory/perf.md`.
