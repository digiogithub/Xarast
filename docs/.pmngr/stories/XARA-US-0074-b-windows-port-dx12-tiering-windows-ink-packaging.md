---
id: XARA-US-0074
type: story
title: "B — Windows port: DX12 tiering, Windows Ink, packaging"
status: backlog
parent: XARA-EP-0015
author: mcp
labels: [phase-14, windows]
created: 2026-09-23T09:43:25Z
updated: 2026-09-23T09:43:25Z
---

## Tasks (full table: phase-14 §B)
- B1 DX12 → Vulkan → GLES → `vello_cpu` tiers.
- B2 `WGPU_BACKEND`/`XARAST_RENDERER` escape hatches.
- B3 Windows Ink via winit 0.31 `TabletTool` (depends on winit upgrade).
- B4 Wintab fallback only if B3 fails on hardware.
- Installer and code signing.
