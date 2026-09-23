---
id: XARA-T-0155
type: task
title: .xarast round trip turns a duotone (contone) bitmap fill into a flat colour
status: backlog
priority: low
parent: XARA-US-0052
author: mcp
labels: [phase-10, bitmap, xarast-format]
created: 2026-09-23T17:48:11Z
updated: 2026-09-23T17:48:11Z
---

## Description
Now that bitmaps decode, `Designs/Fill Types simple.xar`'s bitmap row renders from the `.xar`: plain, repeating, repeating inverted, contoned red-white and duotoned alt-rainbow. After `.xar → .xarast → reload`:
- "Duotoned alt-rainbow" renders as a flat green square (the contone pair and/or its effect are lost, or the reader falls back to the flat approximation);
- "Repeating inverted" tiles without mirroring (the fill-mapping gap, XARA-T-0109).

Both make `Fill Types simple.xar` a `KNOWN_RENDER_GAPS` entry in `xarast-app/tests/xarast_roundtrip.rs` (255 levels on 0.79 % of pixels).

## Acceptance Criteria
- A contone/duotone bitmap fill round-trips its two colours and fill effect; the cell renders the same after reload.
- With XARA-T-0109 fixed, `Fill Types simple.xar` leaves `KNOWN_RENDER_GAPS`.

## Notes
Headless evidence: `xarast-cli convert` then `xarast-cli render` of the package versus the `.xar`.
