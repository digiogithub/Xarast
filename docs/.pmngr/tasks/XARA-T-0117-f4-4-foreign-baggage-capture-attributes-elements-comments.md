---
id: XARA-T-0117
type: task
title: "F4.4 Foreign baggage capture: attributes, elements, comments, PIs with positions"
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
Every attribute an element kind does not interpret, every unknown child element (verbatim text plus the namespace declarations it uses), unknown SVG elements (with a warning when they render), comments and PIs go into the node's `ForeignBaggage` (T-0089 store) with their position among the known children; unknown `style` declarations and foreign classes are kept; the root's attributes and prolog comments go on the root. Active content is stripped.

## Notes
Done in 967da9a. Tests: `tests/svg_read.rs::foreign_data_added_by_another_editor_survives_in_place`, `active_content_is_stripped`, `a_foreign_class_is_kept`; Inkscape 1.2 round trip recorded in `docs/memory/xarast-format.md`. Header data with no node (namedview, metadata, run attributes, foreign ids): XARA-T-0112.
