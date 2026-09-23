---
id: XARA-T-0131
type: task
title: T1.4 — Labelled Undo/Redo in a new Edit menu (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y)
status: done
parent: XARA-US-0029
author: mcp
labels: [phase-7, ui]
created: 2026-09-23T17:14:06Z
updated: 2026-09-23T17:14:06Z
---

## Description
History/CommandBus undo_label/redo_label; AppCommand Undo/Redo/Delete/SelectAll/Cancel; Edit menu shows "Undo Move".

## Notes
Commits dc4786c, 2a4f8e8, f777477, 7c172a7. Tests: xarast-ui/tests/toolbar.rs, viewer select_drag_undo_and_redo_in_the_whole_viewer.
