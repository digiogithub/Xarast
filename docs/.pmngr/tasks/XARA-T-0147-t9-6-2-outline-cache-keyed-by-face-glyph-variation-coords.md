---
id: XARA-T-0147
type: task
title: T9.6.2 — Outline cache keyed by (face, glyph, variation coords)
status: done
parent: XARA-US-0049
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:43Z
updated: 2026-09-23T17:24:43Z
---

## Description
Cache inside `FontDb` keyed by (face, glyph, normalised coords with trailing zeros stripped); scaling is a transform, never a re-extraction. Cleared wholesale at 65 536 entries.

## Acceptance Criteria
- Same `Arc` on repeat and for `[]` vs `[0]`; cached lookup 24 ns (budget ≤ 500 ns).

## Notes
Commit 8a96272, bench 4e243b2.
