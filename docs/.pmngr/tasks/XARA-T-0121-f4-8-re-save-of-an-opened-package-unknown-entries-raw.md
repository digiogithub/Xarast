---
id: XARA-T-0121
type: task
title: "F4.8 Re-save of an opened package: unknown entries raw-copied, conservative GC"
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
`save_opened` / `save_opened_to`: `ResourceIndex::from_package` + recount through `write_svg`, `mark_referenced_in` over every baggage attribute and fragment, `gc`, raw copies of unchanged resources, `carry_from` for unknown entries, atomically.

## Notes
Done in 71fe240; `tests/svg_read.rs::unknown_entries_and_resources_named_only_by_foreign_data_survive_a_re_save`.
