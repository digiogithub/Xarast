---
id: XARA-T-0249
type: task
title: "T11.5.3 follow-up: CMYK output with a profile (PDF DeviceCMYK/ICCBased, SVG icc-color, our own sRGB profile)"
status: backlog
parent: XARA-US-0060
author: mcp
labels: [phase-11, io, colour]
created: 2026-09-23T23:01:28Z
updated: 2026-09-23T23:01:28Z
---

## Description
XARA-US-0060 made every export report CMYK and spot colours as `Compromise::ColourConverted` (written as their naive sRGB conversion) and image ICC profiles not applied as `ProfileDropped` (raster, PDF). What remains of T11.5.3:

- **PDF `DeviceCMYK` for flat CMYK fills** (an option, off by default): PDF's own DeviceCMYK→RGB rule `1 − min(1, C + K)` is exactly our conversion, so screen appearance is unchanged and separations survive. Blocker: the walker resolves colours to `Rgba8`, so the CMYK value never reaches the display list; it needs a side channel keyed by scene node or a paint variant.
- **An sRGB ICC profile of our own** (generated from the sRGB primaries and curve, no third-party profile bytes): the PDF output intent (XARA-T-0230), an optional WebP `ICCP`, and the prerequisite for `ICCBased` spaces.
- **SVG `icc-color()`** once a CMYK profile exists (research/06 §6.12.3: only with a profile), and `DeviceN` for spot inks in PDF.

## Acceptance Criteria
- A flat CMYK fill exported to PDF with the option on renders identically in Poppler and keeps its CMYK operands (`k`); the colour-sheet test covers it.
- The report stops listing `ColourConverted` for what the output now carries.
