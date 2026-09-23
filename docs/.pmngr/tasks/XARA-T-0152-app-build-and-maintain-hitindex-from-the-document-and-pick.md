---
id: XARA-T-0152
type: task
title: "App: build and maintain HitIndex from the document and pick with HitShape (HitTester::pick / pick_all)"
status: todo
parent: XARA-US-0031
author: mcp
labels: [phase-7, app-core]
created: 2026-09-23T17:41:32Z
updated: 2026-09-23T17:41:32Z
---

## Description

Replace the bounding-box `Session::select_in_rect` and implement `HitTester::pick` / `pick_all` (phase-07 §W3 T3.1–T3.3) on top of the geometry API delivered in XARA-T-0148 and XARA-T-0149. The full contract is in `docs/memory/geometry.md`, section "Integration contract for `xarast-app`". In short:

- Index every selectable ink **leaf** on visible, unlocked, non-guide layers in `xarast_geom::HitIndex<NodeId>`. Bounds come from the bounds cache (they must include the stroke's reach). Z is the render-order rank `<< 16`; rebuild when no gap is left.
- Build with `HitIndex::from_entries` on load and on `InvalidateAll`. Update on each committed transaction (`set_bounds` / `insert` / `remove` / `set_z`), never on each drag frame. Hidden or locked layers: `retain`, or rebuild.
- Pick: `candidates_at(p, radius)` (nearest the viewer first). For each candidate, build a `HitShape` from the path and the resolved attributes (fill only if painted, stroke only if the line colour is not none, the transform for text and images) and call `hit(p, HitTolerance::from_device(px, mp_per_px))`. The first hit wins. Then apply TopGroup, Leaf or Under via the tree's ancestors and z.
- Marquee: `query_rect(rect, RectMode::Touch | Enclose)` on the leaves, then map to top groups. For Enclose in top-group mode, test the group's own bounds.

## Acceptance Criteria

- [ ] Clicking a transparent interior of an unfilled shape does not select it; clicking its outline does, within the device tolerance at any zoom
- [ ] Ctrl-click leaf, Alt-click under, locked and hidden layers never picked
- [ ] Marquee touch and enclose over groups
- [ ] Index updated incrementally on commit; a test asserts index == rebuilt index after a random edit sequence
- [ ] Golden hit-test suite (phase-07 criterion 18)

## Notes

Measured on the reference machine: a precise pick among 100k objects takes 5.5–13.8 µs, and a whole-page marquee ≤ 1.38 ms. The adversarial case (100k unfilled outlines all around the point) takes 18–21 ms because every candidate needs a precise rejection. If real documents hit that case, cache per-object "hollow" facts or cap rejections on this side.
