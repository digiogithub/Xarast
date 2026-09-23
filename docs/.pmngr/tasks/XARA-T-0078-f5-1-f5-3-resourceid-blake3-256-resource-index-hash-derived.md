---
id: XARA-T-0078
type: task
title: F5.1–F5.3 — ResourceId (BLAKE3-256), resource index, hash-derived names
status: done
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:23:15Z
updated: 2026-09-23T14:23:15Z
---

## Description
`ResourceId`, `ResourceIndex::{insert, insert_reader (hash while streaming), from_package}`, `resource_path`/`parse_resource_path` (`resources/<dir>/b3-<32 hex>.<ext>`, allowed extensions only). Records from the package stay `ResourceData::Package` and are raw-copied on save: no rehash, no recompression.

## Acceptance Criteria
- `eight_uses_one_entry` (1 entry, bytes_saved > 7×len − 4096, refcount 8); `resave_from_the_package_is_a_fixed_point`.

## Notes
Done in commit 0529f6d.
