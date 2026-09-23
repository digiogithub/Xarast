---
id: XARA-US-0018
type: story
title: As a user, blend modes match the original's output
status: backlog
parent: XARA-EP-0018
author: mcp
labels: [render, research]
estimate: 5
created: 2026-09-23T09:40:52Z
updated: 2026-09-23T09:40:52Z
---

## Description
Needs an x86-64 VM running the original (black-box observation only — clean room).

## Acceptance Criteria
- Luminance weights recovered by least squares (R4.4); twelve tables extracted via `GDraw::CalcTransparencyX` (R4.5).
- Contrast, Bevel, Saturation and Luminosity verified against them.
- Corpus rendered end to end and compared at 25/100/400 %; cache admission threshold re-derived from that data.
