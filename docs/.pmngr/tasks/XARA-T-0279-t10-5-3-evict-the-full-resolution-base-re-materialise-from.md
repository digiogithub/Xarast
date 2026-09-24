---
id: XARA-T-0279
type: task
title: T10.5.3 Evict the full-resolution base; re-materialise from the original or a spill file
status: in_review
parent: XARA-US-0053
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T09:36:59Z
updated: 2026-09-24T10:11:54Z
started: 2026-09-24T09:36:59Z
---

## Description
Evict the base level and the levels above the proxy under budget pressure; re-materialise them on demand from the encoded original (re-decode) or from a spill file, byte for byte.

## Acceptance Criteria
- Corpus files with bitmaps (Groucho2, leafgirl, ...) render byte-identically under a tiny budget.
