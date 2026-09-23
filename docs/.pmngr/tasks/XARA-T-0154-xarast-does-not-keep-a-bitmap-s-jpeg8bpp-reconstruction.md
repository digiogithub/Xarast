---
id: XARA-T-0154
type: task
title: .xarast does not keep a bitmap's JPEG8BPP reconstruction palette
status: done
priority: low
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, xarast-format]
created: 2026-09-23T17:48:11Z
updated: 2026-09-23T18:39:35Z
started: 2026-09-23T18:08:36Z
closed: 2026-09-23T18:39:35Z
---

## Description
Since T10.2.6 the `.xar` importer keeps a tag-71 (`TAG_DEFINEBITMAP_JPEG8BPP`) palette in `BitmapResource::pixels.palette` (pixels empty, original = the 24 bpp JPEG), and the walker decodes such a resource with `xarast_image::xar::decode_xar_bitmap(71, bytes, palette, …)`, snapping the JPEG back onto the 8 bpp palette.

The `.xarast` writer emits only the JPEG bytes, so after a round trip the palette is gone and the bitmap renders as the plain 24 bpp JPEG. Visible in `xarast-app/tests/xarast_roundtrip.rs`: `Designs/leafgirl.xar` (up to 31 levels on 8 % of pixels) and `Designs/Groucho2.xar` (6 levels, 0.8 %), both listed in `KNOWN_RENDER_GAPS`.

## Acceptance Criteria
- The package records the palette of a bitmap resource that has one (e.g. a `xarast:palette` attribute or a sidecar), and the reader restores it into `BitmapData::palette`.
- `leafgirl` and `Groucho2` leave `KNOWN_RENDER_GAPS`.

## Notes
Palette facts: `docs/research/01-xar-format.md` §4.5; decoder contract: `docs/memory/image.md`.
