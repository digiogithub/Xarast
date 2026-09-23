---
id: XARA-US-0023
type: story
title: W3 — SVG profile writer (browser/Inkscape-readable)
status: in_review
priority: high
parent: XARA-EP-0007
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T15:43:35Z
started: 2026-09-23T15:05:06Z
---

## Description
As a user, a `.xarast` opens with graceful degradation in a browser or Inkscape.

## Tasks (full table: phase-06 §W3)
- F3.1 `<svg>` header, `viewBox` in points, `xarast:document y-axis="down"`.
- F3.2 Spreads/pages/layers → `<g>` with `xarast:` roles.
- F3.3 Geometry elements; parametric shapes keep a `xarast:` sidecar.
- F3.4 Compact path-data serialiser.
