---
id: XARA-T-0157
type: task
title: scope3 simple renders differently after a .xarast round trip once bitmaps decode (9 % of pixels, ≤ 4 levels)
status: done
priority: high
parent: XARA-US-0028
author: mcp
labels: [xarast-format, bitmap, round-trip]
created: 2026-09-23T18:03:46Z
updated: 2026-09-23T18:39:35Z
started: 2026-09-23T18:08:36Z
closed: 2026-09-23T18:39:35Z
---

Found when merging the exact-twins writer fix (T-0108…T-0111) with the bitmap wiring (T-0129): the corpus render round trip is pixel-identical for 56/59; Groucho2 and leafgirl differ because of T-0154 (JPEG8BPP palette not saved); scope3 simple differs on 9.08 % of pixels by up to 4 levels — cause unknown (candidates: bitmap fill/transparency parameters, contone, mapping). Listed in `KNOWN_RENDER_GAPS` in `crates/xarast-app/tests/xarast_roundtrip.rs`; the list must end empty.
