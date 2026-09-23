---
id: XARA-T-0051
type: task
title: "Deferred: WGSL paint/blend compositing pass (R5.2–R5.4), re-open on the trigger in render.md"
status: backlog
parent: XARA-US-0011
author: mcp
labels: [render, gpu]
created: 2026-09-23T12:25:01Z
updated: 2026-09-23T12:25:01Z
---

## Description
Option (b) of the XARA-US-0011 decision: evaluate paints and the twelve blend families on the GPU over CPU coverage. Not built now. Its floor is the coverage upload: 64 MiB of A8 costs 27–33 ms on the Intel iGPU and 5.4–6.9 ms on the RTX 4000 Ada, and a gradient-heavy corpus frame composites 1.4 × 10^8 px, so on the iGPU it cannot reach an interactive frame for exactly the documents it would help. It would also add a perceptual-only parity gate for gradients (risk K5) and cannot serve export, which stays on the deterministic CPU path.

## Acceptance Criteria (to re-open)
- Re-open when, after XARA-T-0038 (row-wise SIMD paint/blend), a gradient-heavy corpus file still takes > 100 ms for an interactive Final frame at 1080p on the reference machine, **and** a design keeps coverage on the GPU (or uploads ≤ 16 MiB per frame).
- Then: tile planner, WGSL pass, ping-pong destination reads, parity per gradient cell within 1/255.
