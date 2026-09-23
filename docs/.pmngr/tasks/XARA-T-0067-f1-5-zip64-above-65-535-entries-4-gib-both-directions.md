---
id: XARA-T-0067
type: task
title: F1.5 — ZIP64 above 65,535 entries / 4 GiB, both directions
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
Writer sets `large_file` near 4 GiB; `zip` writes the ZIP64 end record past 65,535 entries; the end-record pre-check parses ZIP64 locator and record.

## Acceptance Criteria
- `zip64_above_65535_entries` (66,004 entries written and reopened). The 4 GiB single-entry path is not exercised in CI (size).

## Notes
Done in commit 0529f6d.
