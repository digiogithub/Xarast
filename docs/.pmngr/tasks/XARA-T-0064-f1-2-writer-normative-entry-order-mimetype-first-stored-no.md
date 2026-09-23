---
id: XARA-T-0064
type: task
title: "F1.2 — Writer: normative entry order, mimetype first, STORED, no extra field"
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
`PackageWriter` buffers entries and writes in one pass: mimetype, manifest, meta, document, thumbnail, previews, resources, history, extensions, others; byte order within groups.

## Acceptance Criteria
- `tests/container.rs::magic_bytes_at_fixed_offsets`: 0..4 = PK\3\4, 28..30 = 00 00, 30..38 = mimetype, 38..64 = MIME.

## Notes
Done in commit 0529f6d.
