---
id: XARA-T-0311
type: task
title: Soak the GPU device-loss rebuild under a real driver reset and a suspend cycle
status: backlog
parent: XARA-US-0064
author: mcp
labels: [phase-12, stability, gpu]
created: 2026-09-24T17:46:34Z
updated: 2026-09-24T17:46:34Z
---

## Description
The device-loss rebuild (shell decision 43 in `ui.md`) is tested only with a simulated loss: `GpuErrorSink::record_lost`, `wgpu`'s no-op backend and `XARAST_INJECT_DEVICE_LOSS=N`. Nobody has checked it against a real driver reset or a suspend/resume cycle. It is also unknown whether `wgpu` 30 reports a suspend-cycle loss through the device-lost callback or only as surface errors.

## Acceptance Criteria
- On the reference machine (by the maintainer, not an agent: this means resetting a real GPU), run a driver-reset script and a suspend/resume cycle while a document is open. Record whether the loss is reported, whether the window comes back, and what the status line says.
- If a loss arrives only as `Lost`/`Validation` surface errors, feed that path into `DeviceLoss` too.

## Notes
Phase 12 risk table: "GPU device loss handling (F2) is hard to test". Filed from XARA-US-0064.
