---
id: XARA-T-0130
type: task
title: T1.1/T1.3 — EditCommand on the CommandBus with per-kind coalescing
status: done
parent: XARA-US-0029
author: mcp
labels: [phase-7, app-core]
created: 2026-09-23T17:13:51Z
updated: 2026-09-23T17:13:51Z
---

## Description
`xarast_app::EditCommand` (TransformNodes, DeleteNodes) implementing `xarast_doc::Command`; `Session::apply_edit` coalesces inside a gesture only when `coalesces_with` allows. Reuses `xarast_doc::Tx` (T1.2 already equivalent).

## Notes
Commits dad9efe, 87db537 (F4.7 marks preserved). Undo bench: ~0.17 ms per undo at 250k nodes.
