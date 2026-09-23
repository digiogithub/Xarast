---
id: XARA-T-0132
type: task
title: Per-node ContentHash (tag + content revision + attribute-scope fold)
status: done
parent: XARA-US-0029
author: mcp
labels: [phase-7, app-core]
created: 2026-09-23T17:14:06Z
updated: 2026-09-23T17:14:06Z
---

## Description
Replace the coarse tag+epoch ContentHash. `Tree::content_rev` (lazy side table, per-tree counter, bumped by every action incl. undo; restore continues numbering); walker folds attribute-node versions per scope.

## Notes
Commits dc4786c, dad9efe. Test an_edit_changes_only_the_content_hash_of_what_it_touched. XARA-T-0053 did not bite (not addressed).
