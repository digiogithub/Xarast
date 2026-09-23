---
id: XARA-T-0143
type: task
title: T9.3.2 — FontMetrics for the document layer
status: done
parent: XARA-US-0046
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:42Z
updated: 2026-09-23T17:24:42Z
---

## Description
`trait FontMetrics { char_metrics(font, size, aspect, c) -> CharMetrics; kern_pair(font, size, l, r) -> Mp }`, implemented by `Shaper` (the FormatRegion replacement). `FaceMetrics::read` (skrifa) with `scaled(size)` in millipoints. Keyed by `FontQuery` rather than `TypefaceRef` so xarast-text does not depend on xarast-doc; the bridge is in docs/memory/text.md.

## Acceptance Criteria
- `char_metrics_scale_with_size_and_aspect`, kern pair A/V = -400 mp at 10 pt.

## Notes
Commit f7c05dd.
