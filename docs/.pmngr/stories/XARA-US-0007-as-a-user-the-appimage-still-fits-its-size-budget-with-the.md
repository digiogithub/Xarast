---
id: XARA-US-0007
type: story
title: As a user, the AppImage still fits its size budget with the full UI stack
status: backlog
parent: XARA-EP-0006
author: mcp
labels: [phase-5, packaging]
estimate: 1
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T09:40:08Z
---

## Description
3.9 MB did not survive phase 5 (wgpu, egui, winit, portal, clipboard, fonts). Re-measure.

## Acceptance Criteria
- AppImage size measured and ≤ 80 MB; recorded in `docs/memory/packaging.md`.
