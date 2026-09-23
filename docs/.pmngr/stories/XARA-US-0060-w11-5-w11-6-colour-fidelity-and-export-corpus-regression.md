---
id: XARA-US-0060
type: story
title: W11.5/W11.6 — Colour fidelity and export corpus regression
status: in_review
parent: XARA-EP-0012
author: mcp
labels: [phase-11, io, acceptance]
created: 2026-09-23T09:42:40Z
updated: 2026-09-23T23:06:16Z
started: 2026-09-23T22:28:11Z
---

## Tasks (full tables: phase-11 §W11.5, §W11.6)
- T11.5.1–4 sRGB markers, ICC pass-through, CMYK handling, fidelity-compromise report.
- T11.6.1 Export all corpus files to all five formats.
- T11.6.2 SVG round-trip via `usvg`.
- T11.6.3 `qpdf --check` + external render compare.
- T11.6.4 Byte reproducibility across runs and machines.
- Render the `.xar` corpus vs the original at 25/100/400 % (render TODO 5).
