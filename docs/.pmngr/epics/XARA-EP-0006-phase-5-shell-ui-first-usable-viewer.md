---
id: XARA-EP-0006
type: epic
title: "Phase 5 — Shell & UI: first usable viewer"
status: in_progress
priority: critical
milestone: XARA-M-0003
author: mcp
labels: [phase-5]
created: 2026-09-23T09:39:43Z
updated: 2026-09-23T09:39:43Z
started: 2026-09-23T09:39:43Z
---

## Description
`xarast-shell`, `xarast-ui`, `xarast-app`: canvas, viewport, layer and colour panels, Wayland integration, portals, clipboard, DnD, input and tablet. Goal: first usable viewer.
Spec: `docs/phases/phase-05-shell-and-ui.md`. Memory: `docs/memory/ui.md`, `docs/memory/app-core.md`.

## Status (2026-09-23)
- 758 tests green, all gates green; `xarast --selftest-window` exits 0 headless.
- Verdicts closed: arena (HAMT lost), egui density (passes), winit (stay on 0.30 — no `accesskit_winit` supports 0.31; winit-x11 has no tablet code).
- **Missing:** the `ShellEvent → Intent` bridge. The three crates are correct separately but not wired into a running application.

## Acceptance Criteria
- A corpus `.xar` opens in the running app and can be panned and zoomed.
- Phase-5 performance budgets measured on real hardware.
