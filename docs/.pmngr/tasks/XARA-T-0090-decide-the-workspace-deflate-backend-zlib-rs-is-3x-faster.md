---
id: XARA-T-0090
type: task
title: Decide the workspace DEFLATE backend (zlib-rs is ~3x faster for .xarast save)
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format, perf]
created: 2026-09-23T14:55:15Z
updated: 2026-09-23T16:23:40Z
started: 2026-09-23T15:52:06Z
closed: 2026-09-23T16:23:40Z
---

## Description
flate2 selects one backend per build, so `zip`'s `deflate-flate2-zlib-rs` switched the backend for every crate in workspace builds and broke `crates/xarast-xar/tests/fuzz_seeds.rs` (the committed seed `fuzz/corpus/fuzz_xar_import/two-blocks-streamed-record.bin` is compressed bytes). The workspace was put back on the single default `miniz_oxide` backend. Measured on the reference machine: save of a 20 MB package 273 ms (miniz_oxide) vs 92 ms (zlib-rs).

Option: enable `flate2/zlib-rs` at the workspace level (so every binary uses the same backend), regenerate that `.xar` seed (needs the xar owner), and update the digest in `xarast-format/tests/container.rs::deterministic_bytes_are_pinned`.

## Acceptance Criteria
- One backend for all crates; xar seed test and the xarast golden test both pass in `cargo test --workspace` and `cargo test -p <crate>`.

## Notes
Only worth doing if the SVG serialiser (W3) eats into the 1 s save budget. Numbers in docs/memory/perf.md.
