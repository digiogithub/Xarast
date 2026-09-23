---
id: XARA-T-0072
type: task
title: F2.2 — Root "/" row; one row per ZIP entry
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
The writer emits exactly one `/` row plus a row for every entry including `mimetype` and the manifest itself (no digest, no size). The reader diagnoses a missing/duplicate/mismatched root row (`Diagnostic::BadRootEntry`).

## Acceptance Criteria
- `round_trip_with_every_kind_of_entry` (rows == entries, root_rows == 1).

## Notes
Done in commit 0529f6d.
