---
id: XARA-T-0307
type: task
title: Bitmap transparencies lose their start/end levels and their mode on .xar import
status: in_progress
priority: medium
parent: XARA-US-0018
author: mcp
labels: [render, xar]
created: 2026-09-24T15:40:18Z
updated: 2026-09-24T16:08:20Z
started: 2026-09-24T16:08:20Z
---

## Description
Found during XARA-US-0018 (2026-09-24). `TAG_BITMAPTRANSPARENTFILL` (171) carries two levels and a type byte (`xarast-xar` `decode.rs`: `171 => (FillKind::Bitmap, true, 2, ...)`), but `import.rs::transp_paint` builds `TranspPaint::Bitmap { contone: None, .. }`, dropping both levels and the mode. The walker (`xarast-app` `paint.rs::transparency`) then draws every bitmap transparency in `BlendFamily::Mix`, reading `t` straight from the bitmap's luminance over the full 0..255 range.

So a bitmap transparency in Stained Glass, Bleach or any other family renders as Mix, and one whose levels span less than the full range renders with the wrong contrast.

## Acceptance Criteria
- The importer keeps the two levels and the mode of a bitmap transparency (as the contone pair, or a field of its own).
- The walker composites it in the family of its mode, with `t` remapped from the bitmap's luminance onto the start..end levels (check the direction against `research/01 §8.4`).
- A corpus census (`xarast-cli inspect --fills`) lists bitmap transparencies with their mode; a render test pins the family.
