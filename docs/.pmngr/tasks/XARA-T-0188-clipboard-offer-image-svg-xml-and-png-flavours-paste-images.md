---
id: XARA-T-0188
type: task
title: "Clipboard: offer image/svg+xml and PNG flavours, paste images"
status: backlog
parent: XARA-US-0035
author: mcp
labels: [phase-7, ui]
created: 2026-09-23T19:29:14Z
updated: 2026-09-23T19:29:14Z
---

## Description
`arboard` offers text, HTML and images only, so a copy's SVG goes out as text/plain; Inkscape and browsers expect `image/svg+xml`. Add a data-control / X11 selection writer (MIT/Apache crate) for `image/svg+xml` + `image/png` (rendered selection) and read images on paste (T7.5).

## Acceptance Criteria
Copy in Xarast → paste in Inkscape gives vector objects (checked manually, isolated session); an image on the clipboard pastes as a bitmap object.
