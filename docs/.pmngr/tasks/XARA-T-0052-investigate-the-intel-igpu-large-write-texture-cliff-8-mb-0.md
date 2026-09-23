---
id: XARA-T-0052
type: task
title: Investigate the Intel iGPU large write_texture cliff (8 MB 0.6 ms vs 33 MB 24 ms)
status: backlog
parent: XARA-US-0011
author: mcp
labels: [render, gpu, perf]
created: 2026-09-23T12:25:01Z
updated: 2026-09-23T12:25:01Z
---

## Description
`benches/tiles.rs` (gpu feature) measured on the Intel Arrow Lake iGPU (Mesa 26.1.6): a full 1920×1080 RGBA `Queue::write_texture` + submit + wait takes 0.54–1.6 ms, but 3840×2160 takes 23–27 ms (4.0× the bytes, ~25× the time); 64 MiB of R8 takes 27–33 ms. The RTX 4000 Ada scales linearly (0.6–0.9 ms vs 2.6–3.5 ms). The shell presents every canvas frame through a full-frame `write_texture` today, so a 4K window on the iGPU spends more than the 16 ms frame budget uploading.

## Acceptance Criteria
- Find whether the cliff is wgpu's staging allocation (per-call staging buffer size / mapping) or the driver, e.g. by uploading the same 33 MB in 256 KiB tile pieces and through a persistent staging buffer + `copy_buffer_to_texture`.
- Record the answer in docs/memory/perf.md; XARA-T-0050 removes most large uploads either way.
