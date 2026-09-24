---
id: XARA-T-0258
type: task
title: F3.10a — bake conical, diamond and 3/4-colour fills into SVG geometry
status: in_review
priority: medium
parent: XARA-US-0043
author: mcp
labels: [phase-8, xarast-format, svg]
created: 2026-09-24T00:01:54Z
updated: 2026-09-24T00:06:11Z
started: 2026-09-24T00:01:54Z
---

## Description
The geometry part of XARA-T-0102 (research/06 §5.4.1 strategy 1): conical, diamond, three-colour and four-colour fills — and conical/diamond transparencies — are written as plain SVG geometry in the fill's own frame (`<pattern>` for a fill, `<mask>` for a transparency, `xarast:generated="fill-bake"`), with their twins. Filter/raster baking (fractal, noise, feathers) and the `BakeProvider` trait stay in XARA-T-0102.

## Acceptance Criteria
- Corpus SVG export within the default 4/255 of our PNG for Fill Types simple, WATCH and WATCH2 (limits dropped from `xtask/export-limits-corpus.txt`).
- `.xarast` round trip exact and a byte fixed point on 59/59.
