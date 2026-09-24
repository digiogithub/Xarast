---
id: XARA-T-0299
type: task
title: "perf: render-cache and undo-history byte ceilings, 60-minute soak and idle-CPU gates (phase 12 B3, B4, B6)"
status: backlog
priority: medium
parent: XARA-US-0061
author: mcp
labels: [perf, phase-12]
created: 2026-09-24T13:11:09Z
updated: 2026-09-24T13:11:09Z
---

## Description
Budgets in phase 12 not yet gated because the features or a window are missing: render/bitmap cache ≤ configured bytes + 5 % (B3; the pixel budget of XARA-US-0053 covers bitmaps only), undo history budget in bytes (B4), the 60-minute soak with < 5 % RSS growth (B6, `cargo xtask soak`), idle CPU < 1 % of a core with the window visible.

## Acceptance Criteria
- Each implemented and added as a gate row in `xtask/perf-budgets.txt` (nightly), or recorded as blocked with the reason.
