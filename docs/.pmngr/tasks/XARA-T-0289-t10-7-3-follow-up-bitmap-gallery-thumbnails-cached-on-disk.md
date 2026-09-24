---
id: XARA-T-0289
type: task
title: "T10.7.3 follow-up: bitmap-gallery thumbnails cached on disk by resource hash, and the 100-thumbnail budget measured"
status: backlog
priority: medium
parent: XARA-US-0055
author: mcp
labels: [phase-10, ui, perf]
created: 2026-09-24T11:52:16Z
updated: 2026-09-24T11:52:16Z
---

## Description
XARA-US-0055 makes gallery thumbnails on a background thread and caches them in memory by the resource's content hash (`xarast-app/src/bitmap_gallery.rs`, `Thumbnails`). T10.7.3 also asks for an on-disk cache keyed by that hash (`$XDG_CACHE_HOME/xarast/thumbnails/`), so reopening a document does not decode every bitmap again, and the phase budget "gallery thumbnail generation, 100 resources ≤ 1.5 s total, off the main thread" has not been measured.

## Acceptance Criteria
- Thumbnails are written to and read from a disk cache keyed by the content hash; a corrupt or truncated cache file is ignored and regenerated.
- The cache has a size bound and evicts oldest first.
- A trace or bench shows 100 resources thumbnailed in ≤ 1.5 s off the main thread (record the number in `perf.md`).

## Notes
The thumbnail decode duplicates the walker's decode dispatch on purpose (the walker was being reworked for XARA-T-0281); unify them once that lands.
