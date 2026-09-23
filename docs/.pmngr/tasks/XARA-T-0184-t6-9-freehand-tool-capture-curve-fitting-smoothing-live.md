---
id: XARA-T-0184
type: task
title: "T6.9 Freehand tool: capture, curve fitting, smoothing, live preview"
status: done
parent: XARA-US-0034
author: mcp
labels: [phase-7, tools, geometry]
created: 2026-09-23T19:00:17Z
updated: 2026-09-23T19:00:17Z
---

## Description
xarast_geom::fit_stroke (least squares + corner split + Newton); original tolerance (64+160·S)/zoom mp; smoothing slider; chunked incremental preview; machine replays below-threshold samples so none is dropped. Acceptance 16 test: 4000-sample 200 Hz stroke within tolerance, preview every sample, < 16 ms per sample.

## Notes
Commits aab7d29, 91bb9d2. Tests: xarast-app/tests/freehand.rs.
