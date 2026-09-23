---
id: XARA-T-0106
type: task
title: Automate the §5.6 render-conformance harness (resvg always; Chromium/Inkscape best effort) with SSIM gates
status: todo
priority: medium
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg, ci]
created: 2026-09-23T15:41:04Z
updated: 2026-09-23T15:41:04Z
---

## Description
Round 2 ran the harness by hand (scratch scripts, not committed): convert the 59 files, render each `document.svg` with `cargo xtask svg-render` (resvg), headless Chrome (navigating to the SVG — as `<img>` Chrome's secure static mode blocks the package's resources) and Inkscape, and compare with SSIM. Results are in `docs/memory/xarast-format.md`: resvg vs Xarast's own renderer mean 0.94 over the corpus, Inkscape/Chrome vs resvg 0.94–0.997 on the six checked files.

Caveat for the gate: the Xarast reference does not draw text (Phase 9) or bitmaps (Phase 10), so text- and photo-heavy files score low *because the SVG is more complete*. Gate per document class (§5.6), and skip or reclassify those classes until the reference draws them.

## Acceptance Criteria
- A test/xtask that fails CI below the §5.6 thresholds with resvg; Chrome and Inkscape jobs skip loudly when absent.
