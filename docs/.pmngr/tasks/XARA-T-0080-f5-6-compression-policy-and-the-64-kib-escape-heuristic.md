---
id: XARA-T-0080
type: task
title: F5.6 — Compression policy and the 64 KiB escape heuristic
status: done
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:23:15Z
updated: 2026-09-23T14:23:15Z
---

## Description
`policy::choose`: already-compressed media types STORED, text/XML/ICC/TIFF/BMP DEFLATE, others measured (deflate level 1 on the first 64 KiB, store below 1.05:1). Portable profile only.

## Acceptance Criteria
- `compression_policy` (JPEG/PNG/WebP/WOFF2 stored, SVG/XML deflated, noise blob stored, text blob deflated).

## Notes
Done in commit 0529f6d.
