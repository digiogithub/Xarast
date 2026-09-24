---
id: XARA-T-0253
type: task
title: T8.7.6 — Eyedropper (Ctrl+E) reading from the CPU render surface
status: backlog
parent: XARA-US-0042
author: mcp
labels: [phase-8, app, ui]
created: 2026-09-23T23:50:12Z
updated: 2026-09-23T23:50:12Z
---

## Description
Not done in XARA-US-0042. The eyedropper (`Ctrl+E`, phase-08 §W8.7 T8.7.6) picks a colour from any document pixel of the CPU render surface and applies it like a colour-bar click (`xarast_app::colour_bar::ColourBarOp::Apply` with `ColourSource::Direct`), or starts a colour drag.

## Acceptance Criteria
- `Ctrl+E` then a click on the canvas sets the selection's fill to the pixel's colour (Shift: line), one undo step.
- The sampled value is the Final-quality render, not a Draft.

## Notes
The colour bar's apply/drag paths are the ones to reuse (`docs/memory/colour.md` decisions 26–29).
