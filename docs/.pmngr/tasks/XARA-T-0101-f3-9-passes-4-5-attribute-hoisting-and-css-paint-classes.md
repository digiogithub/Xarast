---
id: XARA-T-0101
type: task
title: F3.9 passes 4–5 — attribute hoisting and CSS paint classes (SVG size and save time for vector-dense documents)
status: todo
priority: medium
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg, perf]
created: 2026-09-23T15:40:55Z
updated: 2026-09-23T15:40:55Z
---

## Description
Passes 1, 2, 3, 6, 7 and 8 of research/06 §4.5.1 are in (eb285b2); 4 (hoist attributes shared by all children of a `<g>`) and 5 (≥ 8 elements sharing ≥ 3 paint properties → `class="cN"` in one `<style>`) are not: every ink element carries its full resolved paint. The spec measures the passes at ~35 % of the SVG.

Measured (release, dev machine): ProbeX16.xar (518 k nodes) → 56 MB SVG, 9.5 MB package; SVG 600 ms + package 935 ms = 1.5 s, over the 1 s save budget. Deflate of the SVG dominates the package part (miniz_oxide; XARA-T-0090 would cut it ~3×). Pass 3's "elide when the parent does not differ" also needs hoisting to be correct.

## Acceptance Criteria
- Passes 4 and 5 semantically neutral (W4 round trip) and each testable alone.
- ProbeX16 save ≤ 1 s; report the SVG shrinkage over the corpus via `SvgWriteStats`.

## Notes
CSS carries paint only (never geometry), so a stripped `<style>` degrades colour and nothing else.
