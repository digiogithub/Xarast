---
id: XARA-US-0003
type: story
title: As a screen-reader user, the app exposes its UI through AccessKit
status: done
parent: XARA-EP-0006
author: mcp
labels: [phase-5, a11y]
estimate: 3
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T13:48:37Z
started: 2026-09-23T11:00:03Z
closed: 2026-09-23T13:48:37Z
---

## Description
AccessKit transport (S10/U4.2) is not wired.

## Acceptance Criteria
- `accesskit_winit 0.29.2` added to `xarast-shell`, paired with `accesskit 0.21.1` (what egui 0.33 resolves; workspace already pins `accesskit = "0.21"`).
- Orca reads panel labels on a real desktop.
