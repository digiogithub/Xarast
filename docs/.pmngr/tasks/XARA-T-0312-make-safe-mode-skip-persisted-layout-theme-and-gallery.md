---
id: XARA-T-0312
type: task
title: Make safe mode skip persisted layout, theme and gallery scans once they exist
status: backlog
parent: XARA-US-0064
author: mcp
labels: [phase-12, stability]
created: 2026-09-24T17:46:34Z
updated: 2026-09-24T17:46:34Z
---

## Description
Safe mode (F5) today means a software adapter where one is installed, CPU canvas composition and preferences reset in memory. The other items F5 lists (no session restore, no gallery scan, plain theme) and the story's "restored layout" have nothing to turn off yet: preferences, the dock layout and themes are not persisted, there are no plugins, and nothing scans a gallery at start.

## Acceptance Criteria
- Whatever lands first among persisted preferences, a persisted dock layout (`LayoutState` already serialises), a theme choice, session restore or a gallery scan at start checks `AppState::safe_mode()` and skips itself.
- A test for each one.

## Notes
Filed from XARA-US-0064 (F5).
