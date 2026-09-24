---
id: XARA-T-0302
type: task
title: Materialise derived photo renditions in .xarast and degrade gracefully in browsers (T10.6.6)
status: backlog
parent: XARA-US-0054
author: mcp
labels: [phase-10, image, xarast-format]
created: 2026-09-24T13:19:22Z
updated: 2026-09-24T13:19:22Z
---

## Description
`.xarast` stores a bitmap object's chain as `<xarast:photo-ops>` inside its `<image>`, whose `href` stays the **master**. Xarast renders it exactly, but a browser or Inkscape shows the master unadjusted, and after a crop or a quarter turn stretched into the object's parallelogram (which maps the derived image).

Implement the materialisation policy of phase 10 T10.6.6 / `research/06 §4.4`: write a derived rendition into `resources/derived/` (with `mf:derived-from` and `mf:derivation` in the manifest) when regenerating it on open would be expensive (> 250 ms at nominal resolution) or the chain holds an unknown op — and consider doing it always for crop/orient so the base SVG draws the right picture. The reader already accepts `xarast:master` on `<xarast:photo-ops>` (the chain's master when `href` names a rendition). Needs a baker closure from the application (the format crate has no image dependency). Also decide the `brightness-contrast` kind of the effect registry (`research/06 §14.3`), which the reader keeps today as an unknown (preserved, not rendered), and an SVG `filter` fallback for tonal-only chains.

## Acceptance Criteria
- A `.xarast` with a cropped/turned/adjusted photo renders in resvg like Xarast's own render (SSIM ≥ 0.99).
- Round trip stays exact (59/59 byte-identical corpus re-save unaffected; derived entries byte-identical across saves).
- The regeneration threshold is measured and recorded in `perf.md`.
