---
id: XARA-T-0145
type: task
title: T9.3.4 — Justification left/right/centre/full with char and space slack
status: done
parent: XARA-US-0046
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:42Z
updated: 2026-09-23T17:24:42Z
---

## Description
Width to the last non-space character without its tracking; right/centre from it; full: spaces get the gap when there are any, letters otherwise (or when the line overflows); only the last tab section stretches; the last line of a wrapped paragraph is left aligned. The remainder is distributed 1 mp at a time so a justified line is exactly its column's width (the original truncates).

## Acceptance Criteria
- Acceptance criteria 6 (exact, 0 mp) and 7 (tracking exclusion, hand-computed) as tests in `tests/layout.rs`.

## Notes
Commit f7c05dd.
