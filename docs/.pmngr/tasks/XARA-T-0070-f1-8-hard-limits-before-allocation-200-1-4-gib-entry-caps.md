---
id: XARA-T-0070
type: task
title: F1.8 — Hard limits before allocation (200:1, 4 GiB, entry caps)
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
`Limits` (normative defaults + `FUZZ`). End record parsed before `zip` allocates (count cap, count×46 ≤ CD size, single disk, no forged record in the comment). Ratio per entry with a 1 MiB floor, total cap, manifest cap 16 MiB, reads capped at declared size + 1. Encryption rejected.

## Acceptance Criteria
- `encrypted_zip_bombs_and_totals_are_rejected`, eocd unit tests.

## Notes
Done in commit 0529f6d. XML depth/DTD/entity limits are in the manifest parser (F2.1).
