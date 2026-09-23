---
id: XARA-US-0008
type: story
title: "Phase 5 verdicts closed: arena, egui density, winit pinning"
status: done
parent: XARA-EP-0006
author: mcp
labels: [phase-5, decision]
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T09:40:08Z
---

## Description
W1 (egui density spike) and W2 (winit pinning) delivered.

## Notes
- egui passes professional density.
- winit stays on 0.30: no published `accesskit_winit` (0.29–0.34) supports winit 0.31; winit-x11 has no tablet code. All winit types live in one translation module so the switch is cheap.
- Cost of staying: no pen pressure, no Linux trackpad gestures, DnD without drop position.
- `octotablet 0.1.0` rejected: no `dlopen` for `wayland-client`, breaks packaging invariant.
