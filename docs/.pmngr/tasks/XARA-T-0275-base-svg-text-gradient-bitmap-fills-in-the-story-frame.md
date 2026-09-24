---
id: XARA-T-0275
type: task
title: "Base SVG text: gradient/bitmap fills in the story frame, generic family without PANOSE"
status: backlog
priority: medium
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, format]
created: 2026-09-24T08:50:16Z
updated: 2026-09-24T08:50:16Z
---

## Description

Split out of XARA-T-0218, whose font-embedding part is done (WOFF2 subsets + `@font-face`, `xarast:font-embed="denied"`) and whose text-on-a-path part was done by XARA-T-0252. What is left of the base SVG of text (`research/06 §6.7`), all invisible to Xarast (the twin is exact):

1. **Gradient / bitmap fills on text** are written in the spread frame like every ink element, but a run sits under the story's `transform`, so a browser misplaces the gradient (TextCurve's rainbow text: svg 3.06/255 in the corpus export check). Needs a def with the inverse story matrix folded into `gradientTransform` / `patternTransform`.
2. No generic family is guessed from a family name without PANOSE (`svg/text.rs::generic_family` → `sans-serif`).
3. Rule 1 of §6.7 asks for a stable `xarast:font-id` (and the table a `xarast:font-ref` to the `resources/fonts/` file); neither twin is written yet.

## Acceptance Criteria

- A gradient-filled story renders in resvg where Xarast draws it (TextCurve svg under 1/255 on the text).
- The corpus round trips stay 59/59 (model, bytes, render).
