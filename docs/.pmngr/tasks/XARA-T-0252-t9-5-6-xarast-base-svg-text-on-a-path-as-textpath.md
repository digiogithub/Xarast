---
id: XARA-T-0252
type: task
title: "T9.5.6 .xarast base SVG: text on a path as textPath"
status: done
parent: XARA-US-0048
author: mcp
labels: [phase-9, text, format]
created: 2026-09-23T23:43:49Z
updated: 2026-09-24T01:20:40Z
started: 2026-09-24T00:15:48Z
closed: 2026-09-24T01:20:40Z
---

## Description
The `.xarast` writer keeps a story on a path exactly (`xarast:path-params` with the pre-fit character transform, the path as a child), but the base SVG a browser sees still places the characters on straight lines (`SvgStats::text_on_path`, and `svg_text::SvgTextPlacer` ignores the fit). Write `<textPath href>` over the followed path (reversed when the story is), `startOffset` from the left indent, per research/06 §6.7 table row "Text on a path", or place each character with a rotate from `PathFit::cluster_transform`. Same for SVG export in `xarast-io`.

## Acceptance Criteria
- `TextCurve.xar` saved as `.xarast` shows its text along the curves in a browser (resvg render within tolerance of the Xarast render).
- The exact twin still round-trips: 59/59 corpus files render identically after a round trip.
