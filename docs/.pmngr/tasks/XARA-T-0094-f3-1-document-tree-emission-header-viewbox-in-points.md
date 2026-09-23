---
id: XARA-T-0094
type: task
title: "F3.1 — Document tree emission: header, viewBox in points, physical size, namespaces, xarast:document y-axis=\"down\""
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:05Z
updated: 2026-09-23T15:40:05Z
---

## Description
`crates/xarast-format/src/svg/mod.rs::assemble`: XML declaration, the §5.2 namespace block (plus foreign namespaces from baggage), `width`/`height` in mm, `viewBox` in points framing the first spread's pages, root id and baggage, `<title>`, Dublin Core `<metadata>` mirror (subset), `<defs>` with `<xarast:document version min-reader y-axis="down" layout colour-refs [foreign-digest foreign-count]>`, `<xarast:chapter>` rows, `<xarast:palette>`, and `<sodipodi:namedview>` with the first spread's guides and grid.

## Acceptance Criteria
- `tests/svg.rs::the_header_is_the_normative_one`.

## Notes
Done in eb285b2.
