---
id: XARA-T-0205
type: task
title: T8.1.7 — Palette round-trip through .xarast with signed shades and edited palettes
status: backlog
parent: XARA-US-0037
author: mcp
labels: [phase-8, xarast-format]
created: 2026-09-23T19:53:06Z
updated: 2026-09-23T19:53:06Z
---

## Description
`xarast-format` already writes `xarast:kind="shade"` with `xarast:shade="x y"`. After W8.1: shade coordinates are **signed** in `[-1, 1]` (the `.xar` reader keeps the sign via `ColourKind::from_raw`), tints/shades take their parent's model, and `cached_rgb` is the original's packed value. Verify write → read → compare of palettes containing negative shades, links with inherited components and tints of every model; confirm the reader does not clamp shade coordinates to `0..1`.

## Acceptance Criteria
- A palette round-trip test in `xarast-format` over the corpus: every entry's kind, parent, components and resolved `to_rgba8_packed` survive.
