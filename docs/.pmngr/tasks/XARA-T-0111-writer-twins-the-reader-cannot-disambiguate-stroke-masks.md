---
id: XARA-T-0111
type: task
title: "Writer twins the reader cannot disambiguate: stroke masks, lone transparency twins, bitmap transparency images"
status: backlog
priority: medium
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T16:46:55Z
updated: 2026-09-23T16:46:55Z
---

## Description
Three places where what the writer emits cannot be read back unambiguously (found writing the W4 reader, `svg/read/build/paint.rs`):

1. A **graduated stroke transparency** builds a `<mask>` in `<defs>` (`transparency(...)` for the stroke side) that no element references (`emit.rs` only puts `ft.mask` on the element). The stroke's transparency is lost on reload.
2. Fill and stroke transparency twins are both `<xarast:transparency>`. With both a fill and a stroke and **one** twin, the reader cannot tell whose it is; it assumes the fill's.
3. A **bitmap transparency** twin (`xarast:type="bitmap"`) records the control points but not the image: the reader substitutes an empty placeholder bitmap (the renderer has no decoded pixels yet either, so nothing shows today).

## Acceptance Criteria
- The stroke mask is either referenced (a `mask` on a stroke-only duplicate, or a `xarast:stroke-mask` twin) or not built.
- The stroke's twin is distinguishable (e.g. `<xarast:stroke-transparency>`); the reader accepts both spellings.
- A bitmap transparency twin carries `href`/`xlink:href` to its image resource (and counts a reference).
