---
id: XARA-T-0063
type: task
title: F1.1 — zip 8.x wiring, DEFLATE pinned to zlib-rs
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:13Z
updated: 2026-09-23T14:22:13Z
---

## Description
`zip` 8.6 (MIT; 9.0 is still a pre-release) with every default feature off and only `deflate-flate2-zlib-rs`, so the reader has no AES/bzip2/lzma/xz/ppmd/deflate64 surface and DEFLATE output is reproducible for a given lock file. Zstd is not compiled in (compact profile is v1.0). `quick-xml` 0.41 (already in the graph), `blake3` 1.8.

## Acceptance Criteria
- `cargo deny check licenses` passes.

## Notes
Done in commit 0529f6d.
