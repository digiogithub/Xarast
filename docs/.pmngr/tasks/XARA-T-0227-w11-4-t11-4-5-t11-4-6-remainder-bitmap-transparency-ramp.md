---
id: XARA-T-0227
type: task
title: "W11.4 T11.4.5/T11.4.6 remainder: bitmap transparency, ramp alpha, layer masks, per-family blend ΔE"
status: backlog
priority: medium
parent: XARA-US-0059
author: mcp
labels: [phase-11, io, pdf]
created: 2026-09-23T21:35:10Z
updated: 2026-09-23T21:35:10Z
---

## Description
The PDF exporter (XARA-US-0059 round 1) writes flat opacity and graduated transparency (luminosity soft mask) natively. Still rasterised by the fidelity ladder:
- bitmap-sourced transparency (`TranspSource::Image`),
- gradient ramps / mesh corners whose colours carry alpha,
- layers (`PushLayer`/`PopLayer`) with graduated or bitmap transparency,
- every transparency family other than Mix (Stained Glass and Bleach map to Multiply/Screen only under `BlendFidelity::PreferNative`).

T11.4.6 proper: measure ΔE between each Xara family (`research/03 §2.7.2`, `xarast_render::blend`) and its same-named or same-shaped PDF blend mode on a test patch; decide per family native vs rasterise; record in `docs/memory/render.md`; consider making `PreferNative` the default for the families that pass.

## Acceptance Criteria
- Bitmap transparency and ramp alpha exported as soft masks, no `Compromise::Rasterised` for them.
- The per-family ΔE table exists in `docs/memory/render.md`.
- Phase 11 criterion 10 still holds under `BlendFidelity::Exact`.

## Notes
Code: `crates/xarast-io/src/pdf/mod.rs` (`state_for`, `blend_plan`, `pop_layer`), `pdf/shading.rs` (`paint_opacity`).
