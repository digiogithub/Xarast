---
id: XARA-T-0089
type: task
title: "Doc model: per-node foreign-baggage container (risk K1, prerequisite of W4)"
status: todo
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-doc]
created: 2026-09-23T14:23:38Z
updated: 2026-09-23T14:23:38Z
---

## Description
xarast-doc has no per-node container for foreign baggage (unknown foreign attributes as (uri, local, value), verbatim unknown child fragments with sibling position, comments/PIs, dirty/stale/base-authoritative marks — research/06 §8.2, §8.5). The phase plan (prerequisites, risk K1) says it must exist before the SVG reader, and retrofitting it later costs the whole model. Needs a decision by the doc-model owner: a side table keyed by NodeId (e.g. `SecondaryMap<NodeId, ForeignBaggage>` owned by the document, surviving undo) is the least invasive shape; edits that touch a node must be able to set `foreign-dirty`/`foreign-stale`.

## Acceptance Criteria
- Baggage survives move, recolour, group, change layer, undo, redo (research/06 §8.7 test battery).

## Notes
Filed by the xarast-format owner; not implemented there to stay out of xarast-doc.
