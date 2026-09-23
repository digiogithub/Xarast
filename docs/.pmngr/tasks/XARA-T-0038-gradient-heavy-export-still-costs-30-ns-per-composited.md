---
id: XARA-T-0038
type: task
title: Gradient-heavy export still costs ~30 ns per composited pixel over only three bands
status: backlog
priority: low
parent: XARA-US-0057
author: mcp
labels: [render, perf]
created: 2026-09-23T11:55:47Z
updated: 2026-09-23T12:32:44Z
---

## Description
This is the follow-up to XARA-T-0014. The three `*GradFilledShapes*` corpus files now render at 100 % in 2.6 / 2.2 / 4.5 s, down from 10 s. XARA-T-0014 recorded why they cannot come close to the flat-fill numbers: they composite 1.4–2.1 × 10⁸ pixels (shapes of ~14 000 px each, against ~100 px in the `bulk` bench).

Two costs remain:

1. **Per pixel, about 30 ns**: paint sample ~8 ns (microbench), graduated-transparency sample about the same, blend ~9 ns. Everything is scalar, per pixel, and goes through `Option`/`match`.
2. **Parallelism**: `CpuConfig::deterministic` now uses every core, but a 766 px wide image at 1 MiB per band is only three bands tall. Export therefore runs on three cores.

## Options
- Row-wise evaluation: compute `u` for a whole run of covered pixels in a tight loop, then map the ramp and blend into a scratch row. Keep each pixel's expression bit-identical, or re-bless goldens deliberately.
- A 256-entry fast path for the Mix blend when the destination is opaque.
- More, shorter deterministic bands for narrow images. This changes band geometry: check `determinism::the_band_height_does_not_change_the_pixels` and any hashed corpus renders first. The 100k bulk scene showed 8 of 8.3 M bytes differing by ≤ 2 between band heights.

## Acceptance Criteria
- `xarast-cli render` of each file ≤ 1 s on the reference machine, goldens unchanged or re-blessed with a stated reason.
