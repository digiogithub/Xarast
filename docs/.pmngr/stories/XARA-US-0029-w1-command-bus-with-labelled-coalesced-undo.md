---
id: XARA-US-0029
type: story
title: W1 — Command bus with labelled, coalesced undo
status: in_progress
priority: high
parent: XARA-EP-0008
author: mcp
labels: [phase-7, app-core]
created: 2026-09-23T09:42:03Z
updated: 2026-09-23T16:43:14Z
started: 2026-09-23T16:43:14Z
---

## Description
As a user, every edit is undoable and a whole drag is one undo step ("Undo Move").

## Tasks (full table: phase-07 §W1)
- T1.1 `Command` enum + `CommandBus`; inverse before apply.
- T1.2 `Transaction` builder (drop without commit reverts).
- T1.3 Coalescing policy.
- T1.4 Labelled undo/redo in the menu.
- Replace coarse `ContentHash` (tag + epoch) with a real per-node content hash so the render cache pays off.
