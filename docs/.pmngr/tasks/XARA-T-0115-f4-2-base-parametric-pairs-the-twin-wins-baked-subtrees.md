---
id: XARA-T-0115
type: task
title: "F4.2 Base + parametric pairs: the twin wins; baked subtrees handled"
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
Parallelograms over `d`, quick-shape parameters (the base `d` kept as the outline cache when it differs from `QuickShape::outline()`), live-effect parameter elements, conical/3-4-colour/fractal/noise twins over their flat approximation, `xarast:stops`/`xarast:levels` keys over baked stops. `xarast:generated` subtrees under their controller are kept as generated nodes (nothing regenerates before Phase 13; the writer marks them base-authoritative).

## Notes
Done in 967da9a (`svg/read/build.rs`, `build/ink.rs`, `build/paint.rs`).
