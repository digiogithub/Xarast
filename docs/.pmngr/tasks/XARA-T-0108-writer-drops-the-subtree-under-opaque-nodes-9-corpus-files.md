---
id: XARA-T-0108
type: task
title: Writer drops the subtree under Opaque nodes (9 corpus files lose objects on save)
status: done
priority: high
parent: XARA-US-0028
author: mcp
labels: [phase-6, xarast-format, svg, data-loss]
created: 2026-09-23T16:46:17Z
updated: 2026-09-23T18:01:50Z
started: 2026-09-23T17:10:17Z
closed: 2026-09-23T18:01:50Z
---

## Description
The `.xar` importer keeps an unknown record as `NodeKind::Opaque` **with its children** (the record's subtree: paths, groups). The SVG writer (`svg/emit.rs`, the `NodeKind::Opaque` arm) writes `<xarast:opaque>` with the base64 payload and the node's foreign fragments only — the children are never emitted, so saving loses them. Found by the W4 reader's corpus round trip once the normal form listed Opaque children: ProbeX16, testimp1, BLUECAR, TextCurve, WATCH, WATCH2, Watch4, leafgirl, scope3 simple. In scope3 simple 100 paths and a group vanish (2.3 % of the rendered pixels change).

## Acceptance Criteria
- The writer emits an Opaque node's non-attribute children as elements inside `<xarast:opaque>`, after the payload text (attribute children resolved as for any container).
- The reader already builds element children of `<xarast:opaque>` as children of the Opaque node (W4, `svg/read/build.rs::opaque`).
- `tests/svg_roundtrip.rs`: the `KNOWN_OPAQUE_LOSS` list becomes empty; `crates/xarast-app/tests/xarast_roundtrip.rs`: scope3 leaves `KNOWN_RENDER_GAPS`.

## Notes
Writer-side change; the reader side is done and tolerant of both.
