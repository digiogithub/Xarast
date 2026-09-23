---
id: XARA-T-0092
type: task
title: fuzz_xarast_manifest — cargo-fuzz target for the manifest parser
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format, fuzz]
created: 2026-09-23T14:58:50Z
updated: 2026-09-23T14:58:50Z
---

## Description
`fuzz/fuzz_targets/fuzz_xarast_manifest.rs`: parse arbitrary text; whatever parses and writes must re-parse, keep entry count, capability meaning and version, and be a write–parse fixed point after one round. In the nightly workflow.

## Acceptance Criteria
- Four findings in the first three runs (name validation, BOM offsets ×2, nested fragments moving up a level), all fixed with regression tests; final 10-min run 6.69 M executions, 2 970 edges, clean.

## Notes
Commits 7238459 (target), 0529f6d / ff000cb / 9680a7d (fixes).
