---
id: XARA-T-0096
type: task
title: "F3.3 — Geometry elements; parametric shapes keep a xarast: sidecar"
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:30Z
updated: 2026-09-23T15:40:30Z
---

## Description
Paths → `<path>`; axis-aligned rectangles → `<rect>`; axis-aligned ellipses → `<ellipse>`/`<circle>` (even diameters only, so the centre is exact); rotated/sheared/mirrored ones → `<path>` + `xarast:shape` + `xarast:parallelogram`; quick shapes → generated outline + `<xarast:quickshape>` with every parameter and `<xarast:edge-path>` templates when reformed; bitmaps → `<image>` (x/y/width/height when axis-aligned, matrix otherwise) with `href` and `xlink:href`. `line`/`polyline`/`polygon` are not produced: the model has no such kinds, and a path is exact.

## Notes
Done in eb285b2. The bitmap corner order follows research/01 §4.5 (origin = image top-left); checked visually on Spitfire, SimpleText, leafgirl with resvg, Inkscape and Chrome.
