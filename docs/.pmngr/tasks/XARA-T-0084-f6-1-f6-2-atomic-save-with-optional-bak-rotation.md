---
id: XARA-T-0084
type: task
title: F6.1/F6.2 — Atomic save with optional .bak rotation
status: done
parent: XARA-US-0026
author: mcp
labels: [phase-6, xarast-format, durability]
created: 2026-09-23T14:23:38Z
updated: 2026-09-23T14:23:38Z
---

## Description
`durability::write_atomic[_with]`: O_EXCL temporary `<name>.tmp-<pid>-<rand>` in the target's directory, write, flush, fsync, optional `.bak` (hard link or copy, then rename), rename, directory fsync. Follows symlinks, keeps permissions. The document-level `save_atomic(doc)` is `write_atomic(path, |f| writer.finish_with_source(f, src))` once W3 exists.

## Acceptance Criteria
- `every_fault_leaves_the_original_intact` (7 injected faults + 2 writer failures: original byte-identical, no `.tmp-` left).

## Notes
Done in commit 6e85a50. Deleting autosave/journal after a real save waits for F6.5/F6.6.
