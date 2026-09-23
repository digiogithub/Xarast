---
id: XARA-T-0120
type: task
title: "F4.7 Edit-marking rules: foreign-dirty and foreign-stale set by the commands"
status: done
priority: medium
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-doc]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
`Tx::transform` marks the moved subtree's baggage `DIRTY`; `Tx::set_attr` and attaching an attribute mark the owner's subtree `DIRTY`; `Tx::set_kind` marks an ink node `STALE`, another `DIRTY`; regroup, change layer and delete mark nothing. Undoable with the edit.

## Notes
Done in 5fb94fb; `tests::foreign::edits_mark_the_baggage_they_may_invalidate`. Deletion accounting for the save warning: XARA-T-0113.
