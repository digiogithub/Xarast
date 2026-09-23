---
id: XARA-T-0019
type: task
title: "Fix: NaN ramp stop offsets make the sort panic (fuzz_ramp)"
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, render]
created: 2026-09-23T10:23:55Z
updated: 2026-09-23T10:23:55Z
---

## Description
`build_ramp` / `build_transparency_ramp` sorted stops with `partial_cmp().unwrap_or(Equal)`; with NaN offsets (possible: `Stop`/`TranspStop` fields are public) the comparator is not a total order and std's sort panics ("user-provided comparison function does not correctly implement a total order").

## Fix
NaN offsets map to 0 (as `Stop::new` does) and the sort uses `f32::total_cmp`. Regression test `ramp::tests::nan_offsets_do_not_break_the_sort` (verified to fail before the fix).
