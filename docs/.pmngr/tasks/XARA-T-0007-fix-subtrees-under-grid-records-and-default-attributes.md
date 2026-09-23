---
id: XARA-T-0007
type: task
title: "Fix: subtrees under grid records and default attributes escape the record accounting (fuzz_xar_import)"
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, xar]
created: 2026-09-23T09:58:29Z
updated: 2026-09-23T09:58:32Z
started: 2026-09-23T09:58:29Z
closed: 2026-09-23T09:58:32Z
---

## Description
`fuzz_xar_import` found, minimised to 72 bytes, `GROUP { GRIDRULERORIGIN { GROUP } }`: the grid records (46/47) are consumed by a sibling scan and their children were never visited, so `mapped + skipped + stripped < records_read`. The children of `TAG_CURRENTATTRIBUTES`' defaults had the same hole.

## Acceptance Criteria
- Regression tests in `crates/xarast-xar/tests/fuzz_regressions.rs` (fail before, pass after).
- Both subtrees accounted as skipped in `crates/xarast-xar/src/import.rs`.
