---
id: XARA-T-0218
type: task
title: ".xarast text for browsers: embedded fonts, <textPath>, fills in the story frame"
status: in_progress
priority: medium
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, format]
created: 2026-09-23T20:03:12Z
updated: 2026-09-24T07:43:40Z
started: 2026-09-24T07:43:40Z
---

## Description

Leftovers of XARA-T-0172. The exact twin is complete (59/59 render round trip), but the *base* SVG still falls short of `research/06 §6.7` in three ways a browser shows:

1. **Fonts are not embedded**: §6.7 rule 2 (WOFF2 subset in `resources/fonts/` + `@font-face`, `xarast:font-embed="denied"` when fsType forbids it) is not implemented. Browsers use whatever the family names resolve to.
2. **Text on a path** is written as straight lines (`<textPath>` not used); depends on W9.5.
3. **Gradient / bitmap fills on text** are written in the spread frame (like every ink element) but the run sits under the story's `transform`, so a browser misplaces the gradient. The twin is exact; only the base is off. Needs a def with the inverse story matrix folded into `gradientTransform` / `patternTransform`.

Also: no generic family is guessed from a family name without PANOSE (`svg/text.rs::generic_family` → `sans-serif`).

## Acceptance Criteria

- resvg/Inkscape render of `TextDesigns` with embedded fonts matches the CPU reference without system fonts.
- A gradient-filled story renders in resvg where Xarast draws it.
- The corpus round trips stay 59/59.
