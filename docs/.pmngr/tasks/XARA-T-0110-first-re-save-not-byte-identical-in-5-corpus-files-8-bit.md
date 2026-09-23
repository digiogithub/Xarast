---
id: XARA-T-0110
type: task
title: "First re-save not byte-identical in 5 corpus files: 8-bit twin keys and a statistic of unreferenced bitmaps"
status: done
priority: low
parent: XARA-US-0028
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T16:46:38Z
updated: 2026-09-23T18:01:50Z
started: 2026-09-23T17:10:21Z
closed: 2026-09-23T18:01:50Z
---

## Description
`.xar → .xarast → reload → .xarast` is byte-identical on the first re-save for 54 of 59 corpus files and a fixed point on the second re-save for all 59 (W4, `tests/svg_roundtrip.rs`). The five others:

- **Fill Types simple, Spitfire, WATCH2, Watch4**: a twin's key stops (`xarast:stops`, `xarast:colours`) are 8-bit sRGB, while the model's colours are `f32` (often CMYK/HSV palette colours). Baked stops (profiled/rainbow ramps) and flat approximations (conical mid colour, 3/4-colour mean) are re-sampled from the 8-bit keys on reload and may move by one level. The normal form does not see it (derived data).
- **scope3 simple**: `meta.xml`'s `xarast:bitmaps` counts every `BitmapResource`, including one no element references; the profile writes only referenced bitmaps, so the reload counts one fewer.

## Acceptance Criteria
- Either twin keys carry what the model has (a palette reference, or more precision), or the difference is accepted and documented as is.
- `xarast:bitmaps` counts written (referenced) bitmaps, or the model keeps unreferenced ones through a save.
- `tests/svg_roundtrip.rs`'s bound `identical_first >= 54` is raised accordingly.
