---
id: XARA-T-0060
type: task
title: Recent files store under $XDG_STATE_HOME/xarast/recent
status: done
priority: high
parent: XARA-US-0082
author: mcp
labels: [phase-5, app]
created: 2026-09-23T13:50:18Z
updated: 2026-09-23T14:14:13Z
started: 2026-09-23T13:50:18Z
closed: 2026-09-23T14:14:13Z
---

## Description
Persist recently opened files under `$XDG_STATE_HOME/xarast/recent` (fallback `~/.local/state/xarast/recent`), newest first, at most 10, missing files pruned, atomic write. A corrupt or unreadable store is tolerated (starts empty or keeps the readable entries), never a panic.

## Acceptance Criteria
- Unit tests: round trip, cap, dedupe, pruning, garbage input, unwritable location.
