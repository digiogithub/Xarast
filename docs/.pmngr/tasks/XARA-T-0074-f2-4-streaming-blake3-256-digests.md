---
id: XARA-T-0074
type: task
title: F2.4 — Streaming BLAKE3-256 digests
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
`digest::Digest`, `HashingReader`; every data entry gets a digest on write (not only document/meta/resources). `entry()` verifies, `entry_stream()` verifies at EOF, `verify_all()` for validate.

## Acceptance Criteria
- `digest_mismatch_is_caught_on_read`; BLAKE3 ≥ 1 GB/s per core (bench).

## Notes
Done in commit 0529f6d.
