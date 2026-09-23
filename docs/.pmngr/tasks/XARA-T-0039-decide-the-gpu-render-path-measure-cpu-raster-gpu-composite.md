---
id: XARA-T-0039
type: task
title: "Decide the GPU render path: measure CPU raster + GPU composite vs own WGSL pass vs vello (wgpu 29)"
status: in_progress
parent: XARA-US-0011
author: mcp
labels: [render, gpu]
created: 2026-09-23T12:04:02Z
updated: 2026-09-23T12:04:02Z
started: 2026-09-23T12:04:02Z
---

## Description
Decision record for XARA-US-0011 in docs/memory/render.md (and docs/10-architecture.md if §3.3 changes): (a) CPU raster + GPU presentation/tile compositing, (b) own WGSL compositing pass on wgpu 30, (c) vello on wgpu 29 (downgrade or two wgpu versions). Measure on both GPUs of the reference machine.

## Acceptance Criteria
- Evidence table with medians and spread on Intel Arrow Lake iGPU and RTX 4000 SFF Ada.
- Dependency/binary cost of (c) measured (cargo tree -d, cargo deny, stripped size).
- Parity and capability-ladder consequences stated.
