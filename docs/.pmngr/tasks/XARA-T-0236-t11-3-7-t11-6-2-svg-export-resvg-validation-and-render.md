---
id: XARA-T-0236
type: task
title: "T11.3.7/T11.6.2 — SVG export: resvg validation and render comparison in CI"
status: backlog
parent: XARA-US-0058
author: mcp
labels: [phase-11, io, svg, ci]
created: 2026-09-23T22:10:40Z
updated: 2026-09-23T22:10:40Z
---

## Description
XARA-US-0058 added `cargo xtask svg-check [--interchange] <SVG|DIR>...` (usvg parse + resvg render at 256 px, fails on any `xarast:` text) and measured, by hand, resvg renders of the 59-file corpus against our own PNG export at 72 dpi (median mean |Δ| 1.30/255, 52/59 under 4/255; worst: Fill Types simple 20.0, WATCH2 17.0, TextJust 14.9, leafgirl 12.7). resvg is MPL-2.0 and stays in `xtask` (deny.toml), so none of this runs in `cargo test`.

Wire it into CI: export the synthetic fixtures (and the corpus in the nightly job) to SVG, run `svg-check --interchange`, and turn the comparison into an xtask command with per-file thresholds so a regression fails the build. Exclude or separately assert the known gaps: the bake ladder (conical/diamond/3-4-colour fills, XARA-T-0102), resvg pattern tile seams on bitmap fills (leafgirl), and font differences in text.

## Acceptance Criteria
- CI fails when an exported SVG does not parse/render or carries `xarast:`.
- The comparison reports per-file mean |Δ| and fails on a regression beyond a stated tolerance.
