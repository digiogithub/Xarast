---
id: XARA-T-0298
type: task
title: "perf: self-hosted perf runner, history dashboard and required check (phase 12 A5, A7, A8, C2 window)"
status: backlog
priority: medium
parent: XARA-US-0061
author: mcp
labels: [perf, phase-12, ci]
created: 2026-09-24T13:11:09Z
updated: 2026-09-24T13:11:09Z
---

## Description
The `perf` gates (XARA-US-0061) run on shared GitHub runners, headless and CPU-only, with calibration-scaled limits and a retry. Left: (A5) the reference machine as a pinned self-hosted runner so the real-window cold start (≤ 400 ms, XARA-T-0010) and the integrated-GPU pan/zoom p95/p99 can be gated; (A7) a static page over the `perf-*` JSON artefacts; (A8) make the `perf` job a required check; (A6) the three-strike rule across runs (needs the history).

## Acceptance Criteria
- A runner label for the reference machine, a workflow using it for window/GPU gates.
- A per-commit history page built from the artefacts.
- `perf` required for merges to the working branch.
