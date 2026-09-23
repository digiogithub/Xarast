---
id: XARA-US-0005
type: story
title: As a user, drafts refine to final quality when I stop panning
status: done
parent: XARA-EP-0006
author: mcp
labels: [phase-5, render]
estimate: 3
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T11:26:36Z
started: 2026-09-23T10:41:59Z
closed: 2026-09-23T11:26:36Z
---

## Description
Deferred `Draft → Final` upgrade with the 120 ms idle timer and cancellation (render R6.8). Quality levels exist; the scheduler does not. Phase 5 owns the idle timer.

## Acceptance Criteria
- After 120 ms idle, a Final render replaces the Draft; new input cancels it.
