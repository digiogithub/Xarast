---
id: XARA-US-0002
type: story
title: As a user, the UI panels respond to keyboard and mouse through egui
status: backlog
priority: high
parent: XARA-EP-0006
author: mcp
labels: [phase-5, ui]
estimate: 5
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T09:40:08Z
---

## Description
The `egui` shim (S9/U4.1) is not written. Build it directly on `ShellEvent`, not on `winit`, so phase 14 gets it for free.

## Acceptance Criteria
- egui receives pointer, keyboard, IME and scroll input from `ShellEvent`.
- No `winit` type crosses outside `xarast-shell`.
