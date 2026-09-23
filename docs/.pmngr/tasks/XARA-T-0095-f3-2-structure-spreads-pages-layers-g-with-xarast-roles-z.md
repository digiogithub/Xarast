---
id: XARA-T-0095
type: task
title: "F3.2 — Structure: spreads, pages, layers → <g> with xarast: roles; z-order is document order"
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:30Z
updated: 2026-09-23T15:40:30Z
---

## Description
`svg/emit.rs`: first spread as `<g xarast:kind="spread">`, later ones as nested `<svg>` below it (outside the root viewBox), each with `xarast:origin` (document coordinates of its SVG origin, for an exact inverse), size, margin, bleed, double-page, anim. Pages → `<xarast:page xarast:rect>`, grids → `<xarast:grid>`, chapters recorded in `<xarast:document>`. Layers carry `inkscape:groupmode/label`, `xarast:layer-kind`, visible/locked/printable/active, frame props, `sodipodi:insensitive`, `style="display:…"` (guide layers hidden). Groups, clip views, live effects, text stories. One element per non-attribute node, id `x` + base-32 tag, document order.

## Notes
Done in eb285b2.
