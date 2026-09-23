---
id: XARA-T-0011
type: task
title: "Fix: tree builder drops the first group of children of a node descended into twice (fuzz_xar_tree, fuzz_xar_import)"
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, xar]
created: 2026-09-23T10:02:33Z
updated: 2026-09-23T10:02:37Z
started: 2026-09-23T10:02:33Z
closed: 2026-09-23T10:02:37Z
---

## Description
Found independently by `fuzz_xar_tree` (walked nodes 82 vs `tree.nodes` 83) and `fuzz_xar_import` (accounting off by two). For `N DOWN a UP DOWN b UP` the tree builder in `crates/xarast-xar/src/tree.rs` assigned `b` over `a`, silently dropping `a` from the tree while still counting it. Real data loss for a malformed-but-plausible file, not just bookkeeping.

## Acceptance Criteria
- Regression test `a_node_descended_into_twice_keeps_both_groups_of_children`.
- Children appended, per research/01: each `DOWN` makes the following records children of the last node inserted.
