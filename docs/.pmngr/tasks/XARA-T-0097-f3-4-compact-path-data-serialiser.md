---
id: XARA-T-0097
type: task
title: F3.4 — Compact path-data serialiser
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:30Z
updated: 2026-09-23T15:40:30Z
---

## Description
`svg/pathdata.rs` + `svg/num.rs`: integers in millipoints formatted as points with ≤ 3 decimals, no trailing zeros, no leading zero; per-segment absolute/relative choice counting the command letter; collapsed repeats (including L after M); `h`/`v`; `s` for reflected controls; minimal separators. Exponent form is deliberately never written (only shortens round thousands; `1e3mm` is a parser trap).

## Notes
Done in eb285b2. Unit tests in both modules.
