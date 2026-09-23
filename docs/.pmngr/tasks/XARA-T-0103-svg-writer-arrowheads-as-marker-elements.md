---
id: XARA-T-0103
type: task
title: "SVG writer: arrowheads as <marker> elements"
status: todo
priority: low
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:55Z
updated: 2026-09-23T15:40:55Z
---

## Description
Start/end arrows are recorded as `xarast:arrow-start`/`-end` (name or `custom`) but not drawn (`SvgWriteStats::arrows_unbaked`). research/06 §6.6 wants `<marker>` + `marker-start`/`marker-end`, scaled with the line width, plus the name and `xarast:arrow-scale-with-width`. The renderer does not draw arrows either yet, so there is no reference to compare against.
