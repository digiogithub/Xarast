---
id: XARA-T-0066
type: task
title: "F1.4 — Entry-name validation: reject, never sanitise; duplicates rejected"
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
`name::validate_raw_name`: UTF-8, EFS flag for non-ASCII (raw bytes must equal the decoded name), no leading `/`, no `\`, no control chars, no `..`/`.`/empty segments, no drive letter. Duplicates detected by comparing the end record's count with the entries `zip` keeps.

## Acceptance Criteria
- `zip_slip_names_are_rejected`, `duplicate_entries_are_rejected`, `non_ascii_name_without_the_utf8_flag_is_rejected`.

## Notes
Done in commit 0529f6d.
