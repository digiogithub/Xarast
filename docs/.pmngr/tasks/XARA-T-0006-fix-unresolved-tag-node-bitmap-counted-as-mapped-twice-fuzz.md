---
id: XARA-T-0006
type: task
title: "Fix: unresolved TAG_NODE_BITMAP counted as mapped twice (fuzz_xar_import)"
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, xar]
created: 2026-09-23T09:54:13Z
updated: 2026-09-23T09:54:16Z
started: 2026-09-23T09:54:13Z
closed: 2026-09-23T09:54:16Z
---

## Description
First run of `fuzz_xar_import` failed immediately on the committed `every-tag` seed: the record accounting assertion `mapped + skipped + stripped == records_read` was off by one. A `TAG_NODE_BITMAP` (198) whose bitmap reference resolves to nothing becomes an opaque node and was counted as mapped both by the bitmap arm and by `opaque_node`.

## Acceptance Criteria
- Regression test in `crates/xarast-xar/tests/fuzz_regressions.rs`.
- Root cause fixed in `crates/xarast-xar/src/import.rs`.
