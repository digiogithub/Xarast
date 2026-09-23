---
id: XARA-T-0069
type: task
title: F1.7 — Deterministic mode
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
`WriteOptions::deterministic()`: DOS 1980-01-01, 0o644, host system Unix, canonical order, canonical manifest.

## Acceptance Criteria
- `deterministic_writes_are_byte_identical`; an unchanged re-save through `from_package` + `carry_from` is byte-identical.

## Notes
Done in commit 0529f6d. Raw copies take the host system from the running OS (zip limitation), so byte identity holds per platform.
