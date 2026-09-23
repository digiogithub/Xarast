---
id: XARA-US-0004
type: story
title: As a user, the app follows the desktop colour scheme live
status: done
priority: low
parent: XARA-EP-0006
author: mcp
labels: [phase-5, shell, portal]
estimate: 2
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T11:13:01Z
started: 2026-09-23T11:06:17Z
closed: 2026-09-23T11:13:01Z
---

## Description
The settings portal is read once at start-up. Subscribe to the `ashpd` signal stream for live changes.

## Acceptance Criteria
- Switching light/dark in the desktop updates the app without restart.

## Notes
Needs a session bus to test against.
