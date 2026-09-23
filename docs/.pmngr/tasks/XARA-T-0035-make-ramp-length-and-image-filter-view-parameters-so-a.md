---
id: XARA-T-0035
type: task
title: Make ramp length and image filter view parameters so a Draft frame needs no re-walk
status: backlog
priority: low
parent: XARA-US-0005
author: mcp
labels: [render, app, phase-5]
created: 2026-09-23T11:20:26Z
updated: 2026-09-23T11:20:26Z
---

## Description
The Draft/Final scheduler (`xarast_app::schedule`) sets `ViewParams::quality` per frame, which only scales flatness. The other two Draft knobs, the 256-entry ramp and nearest image sampling, are fixed when the walker records the scene (`quality.ramp_length()`, `quality.image_filter()` in `walker.rs`). Switching them per frame would mean re-walking the document at each gesture start and end: ~100 ms for the 250 000-node synthetic document, on exactly the frames that must be cheap. So Draft frames today use Final ramps and image filtering.

## Acceptance Criteria
- The display list or the backend picks ramp length and image filter from `ViewParams::quality`, so one recorded scene serves both qualities.
- The scheduler needs no change; the Draft frames get the cheaper ramps and nearest sampling.

## Notes
Render-crate change; `xarast-app` owns only the scheduler side.
