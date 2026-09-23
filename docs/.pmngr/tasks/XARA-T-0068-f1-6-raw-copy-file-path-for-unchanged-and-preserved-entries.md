---
id: XARA-T-0068
type: task
title: F1.6 — raw_copy_file path for unchanged and preserved entries
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
`PackageWriter::finish_with_source` raw-copies `ResourceData::Package` resources and everything `carry_from` carries (history/, extensions/, unknown entries, non-hash-named resources) with `raw_copy_file_touch`: same compressed bytes, method and CRC.

## Acceptance Criteria
- `unknown_entries_are_carried_byte_for_byte`, `resave_from_the_package_is_a_fixed_point`.

## Notes
Done in commit 0529f6d.
