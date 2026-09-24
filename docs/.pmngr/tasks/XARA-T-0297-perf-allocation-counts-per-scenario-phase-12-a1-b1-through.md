---
id: XARA-T-0297
type: task
title: "perf: allocation counts per scenario (phase 12 A1/B1) through an opt-in dhat feature"
status: backlog
priority: low
parent: XARA-US-0061
author: mcp
labels: [perf, phase-12]
created: 2026-09-24T13:11:09Z
updated: 2026-09-24T13:11:09Z
---

## Description
`xarast-cli bench` reports time and peak RSS but not `alloc_bytes`/allocation counts, the low-noise signal phase 12 A1 asks for. A counting `#[global_allocator]` needs `unsafe`, which phase 0 forbids outside xarast-shell/render/image. Use the `dhat` crate (MIT/Apache) behind `--features profile-alloc` on xarast-cli (B1), record `total_blocks`/`total_bytes`/`max_bytes` in the JSON, and gate them in a separate nightly pass (dhat slows the timed runs).

## Acceptance Criteria
- `cargo build -p xarast-cli --features profile-alloc`; `bench <scenario>` prints allocation metrics.
- `cargo deny` still passes; nightly gates on the counts with a stated tolerance.
