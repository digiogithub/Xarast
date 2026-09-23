---
id: XARA-T-0234
type: task
title: "T11.3.5 — SVG export: user-selectable coordinate precision"
status: backlog
parent: XARA-US-0058
author: mcp
labels: [phase-11, io, svg]
created: 2026-09-23T22:10:40Z
updated: 2026-09-23T22:10:40Z
---

## Description
SVG export writes the profile's numbers: integer millipoints as points with 3 decimals, exact. The phase asks for a user-selectable precision (default 3). Fewer decimals must round in the writer (`svg::num`, `pathdata`) — not in the interchange text projection, because relative path data would accumulate the rounding error. Thread a `decimals` field through `SvgOptions` for the Interchange dialect only; Native stays at 3 (lossless).

## Acceptance Criteria
- `--decimals 0..3` on `xarast-cli export`; 3 is byte-identical to today.
- With 1 decimal, absolute positions stay within 0.05 pt (relative path segments re-anchored so the error does not accumulate); corpus still parses and renders with `cargo xtask svg-check --interchange`.
