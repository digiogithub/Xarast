---
id: XARA-T-0235
type: task
title: "T11.3.3/T11.3.6 remainder — SVG export: Reference resources, physical size, full minify"
status: backlog
parent: XARA-US-0058
author: mcp
labels: [phase-11, io, svg]
created: 2026-09-23T22:10:40Z
updated: 2026-09-23T22:10:40Z
---

## Description
Built in XARA-US-0058: `Inline` (data URIs) and `Sidecar` (`<stem>_files/`, name sanitised to `[A-Za-z0-9._-]`) resources; minify drops unreferenced ids, comments and indentation. Left:
- `SvgResources::Reference`: when exporting over an earlier export, keep the existing relative paths instead of rewriting the files.
- The root `width`/`height` always equal the area's size; honour `ExportSizing`'s physical size (as PDF's `plan_page` does) when one is given.
- Minify: merge `<g>` chains with no attributes of their own, shorten path data (the profile already writes relative/absolute per segment), drop `sodipodi:namedview` and `<metadata>` on request.

## Acceptance Criteria
- Minify stays idempotent (minify twice → identical) and the corpus still renders with `cargo xtask svg-check --interchange`.
- A sized export writes `width`/`height` in the requested unit with the same viewBox.
