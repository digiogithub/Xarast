---
id: XARA-T-0138
type: task
title: T9.1.2 — Arc-shared face data with an LRU cache
status: done
parent: XARA-US-0044
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:20Z
updated: 2026-09-23T17:24:20Z
---

## Description
`FaceData` is a shared (memory-mapped for system faces) blob + face index; `FaceId` is a dense stable index. System faces are kept resident by a bounded LRU (`FontDbOptions::face_cache_capacity`, default 64); embedded faces are always resident.

## Acceptance Criteria
- `face_data_is_shared_not_copied` (same pointer twice); opt-in `the_face_cache_is_bounded` (capacity 2, evicted faces reload).

## Notes
Commit d441e92.
