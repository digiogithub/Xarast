---
id: XARA-T-0079
type: task
title: F5.4 — Refcount GC with preservation and history exemptions
status: done
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:23:15Z
updated: 2026-09-23T14:23:15Z
---

## Description
`retain`/`release`, `begin_recount` + `count_path`, `mark_preserved`, `mark_history`, conservative `mark_referenced_in(foreign_text)`, `gc()`, `retained_unreferenced_bytes()` (risk K9). Initial refcount on open = `mf:refcount` or 1, so nothing is collected before the SVG layer recounts.

## Acceptance Criteria
- `gc_honours_the_exemptions`, `gc_drops_unreferenced_resources_from_the_package`.

## Notes
Done in commit 0529f6d.
