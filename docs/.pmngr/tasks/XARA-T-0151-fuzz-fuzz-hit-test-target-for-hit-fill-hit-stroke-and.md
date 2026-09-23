---
id: XARA-T-0151
type: task
title: "Fuzz: fuzz_hit_test target for hit_fill/hit_stroke and HitIndex, run nightly"
status: done
parent: XARA-US-0031
author: mcp
labels: [phase-7, geometry, fuzz]
created: 2026-09-23T17:39:33Z
updated: 2026-09-23T17:39:33Z
---

## Description

`fuzz/fuzz_targets/fuzz_hit_test.rs`, added to `.github/workflows/fuzz.yml`. Arbitrary paths, transforms (singular, mirroring, up to 1000x), stroke styles, pick points and radii through the precise tests; HitIndex edit sequences against a brute-force list.

## Results

- First run found a timeout/OOM (singular 1000x transform over an extent-sized ellipse: every cubic crossing the probe band was flattened whole at 0.25 mp, 37 s). Fixed by band-clipped flattening (c6e7293); regression test `hit::tests::huge_curves_through_the_band_are_cheap`.
- It also showed that "a larger radius never loses a hit" does not hold on zero-area slivers; not asserted (documented in fuzz/README.md).
- Ten-minute run on the reference machine afterwards: 61 104 execs at ~101 exec/s, clean, peak RSS 1.16 GB.

Commit: 544c492.
