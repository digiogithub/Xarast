---
id: XARA-US-0064
type: story
title: F — Crash reporting, GPU-loss recovery and safe mode
status: in_review
parent: XARA-EP-0013
author: mcp
labels: [phase-12, stability]
created: 2026-09-23T09:43:25Z
updated: 2026-09-24T18:10:35Z
started: 2026-09-24T17:24:32Z
---

## Tasks (full table: phase-12 §F)
- F1 Panic hook writing a local crash report (no document content).
- F2 GPU device-loss recovery with `vello_cpu` fallback.
- F3 Emergency save to `*.xarast.recovered`.
- F4 Startup sentinel.
