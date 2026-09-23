---
id: XARA-T-0134
type: task
title: T2.1–T2.4 — Tool trait, ToolCtx and the shared interaction machine
status: done
parent: XARA-US-0030
author: mcp
labels: [phase-7, app-core]
created: 2026-09-23T17:14:06Z
updated: 2026-09-23T17:14:06Z
---

## Description
`Tool` over a read-only `ToolCtx` (compile-fail doctest), `ToolMachine` Idle→Hover→ArmedDrag→Dragging→Committing/Cancelled, drag threshold, click counting, live modifier re-delivery, edge auto-scroll, cancel on Esc/undo/tool switch; selector, push, zoom; pending stubs.

## Notes
Commits dad9efe, 7c172a7. Tests: xarast-app/tests/tools.rs (18), tool.rs unit tests (4). Momentary-switch keys in the shell remain open.
