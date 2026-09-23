---
id: XARA-US-0025
type: story
title: W5 — Deduplicated resources, compression and thumbnails
status: done
parent: XARA-EP-0007
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T15:01:24Z
started: 2026-09-23T14:22:07Z
closed: 2026-09-23T15:01:24Z
---

## Tasks (full table: phase-06 §W5)
- F5.1 `ResourceId` = BLAKE3-256, streaming on import.
- F5.2 Resource index `hash → (path, refcount)`; no rehash of unchanged resources on save.
- F5.3 Hash-derived names `b3-<hash32>.<ext>`.
- F5.4 Refcount GC with preservation/history exemptions.
- Thumbnail generation (F5.8).
