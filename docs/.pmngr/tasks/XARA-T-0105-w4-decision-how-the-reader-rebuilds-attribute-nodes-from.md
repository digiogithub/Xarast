---
id: XARA-T-0105
type: task
title: "W4 decision: how the reader rebuilds attribute nodes from resolved per-element paint"
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, xarast-doc, svg]
created: 2026-09-23T15:40:55Z
updated: 2026-09-23T17:05:10Z
closed: 2026-09-23T17:05:10Z
---

## Description
The W3 writer (eb285b2) emits **resolved** paint on every ink element and no element for attribute nodes, exactly as the renderer resolves them. Acceptance criterion 4 of phase 6 (`load(save(d)).canonical() == d.canonical()`) cannot hold literally: the model's attribute nodes (lexical scope, positions, redundant repeats, defaults written explicitly by `.xar`) are not in the SVG. Options: (a) the reader localises — one attribute child per non-default slot per object — and the round-trip test compares a normalised form (research/02 §10.6 normalisation boundaries); (b) the writer also records the attribute-node structure in `xarast:` (violates rule 5 and doubles size). Recommend (a), with the normalisation written down and property-tested (render equality + normalised digest).

Also for the reader: non-attribute slots are written only when non-default (`xarast:quality`, overprint, web address, stroke type, width profile, brush, feather); `xarast:filled`/`xarast:stroked="false"` on paths; `xarast:fill-repeat` on non-flat paints; text is laid out approximately (one `<tspan>` per line, runs per style) and TextItems have no ids.
