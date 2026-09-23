---
id: XARA-T-0112
type: task
title: Keep what another editor adds to the SVG header, and foreign ids other data refers to
status: backlog
priority: medium
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg, interop]
created: 2026-09-23T16:46:55Z
updated: 2026-09-23T16:46:55Z
---

## Description
The W4 reader keeps foreign data on every model node, but some places have no node:

1. **`<sodipodi:namedview>`** is regenerated from the model on save; Inkscape's view state (`inkscape:zoom`, `cx`/`cy`, window size, page colour) and guides added in Inkscape are dropped. **`<metadata>`** is regenerated; RDF added there (licence, creator) is dropped. The root `<title>`/`<desc>` beyond the document title likewise.
2. **Text runs** (`<tspan>` inside a line) are not nodes: foreign attributes on them (`sodipodi:role`, Inkscape `style` leftovers) are dropped.
3. **Ids that are not ours** (`path123`, drawn in Inkscape): the object is read and gets a new `x…` id on save. A `<use xlink:href="#path123">` kept as foreign data then points nowhere.

## Acceptance Criteria
- Unknown attributes and children of `namedview` and `metadata` survive a round trip (root-level baggage slots, or model fields for guides).
- A foreign id referenced from foreign data is kept (an alias table, or foreign `id` baggage re-emitted when the node's id would otherwise change).
- Decide and document run-level baggage (merge onto the line, or keep).
