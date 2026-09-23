---
id: XARA-T-0209
type: task
title: T8.2.4 / T8.2.6 — SetFillProfile, SetRampMapping, SetFillEffect, SetTiling, SetTranspMode
status: done
parent: XARA-US-0038
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:53:36Z
updated: 2026-09-23T19:53:36Z
---

## Result
- Commit `48807d8`.
- Profile and mapping edit the ramp (profile also on bitmap/procedural fills; refused on flat and three/four-colour). Effect (`FillEffect` attribute) and tiling (`FillMapping` / `TranspFillMapping`) are separate attributes, as in the format.
- `SetTranspMode` sets the mode of every stop of a transparency fill (the ten modes are values of `TranspMode`).
- All digest-exact on undo/redo (`crates/xarast-doc/tests/fill_palette.rs`).
