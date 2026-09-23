---
id: XARA-T-0248
type: task
title: Render the .xar corpus against the original's own embedded previews (render TODO 5)
status: backlog
parent: XARA-US-0060
author: mcp
labels: [phase-11, render, acceptance]
created: 2026-09-23T23:01:28Z
updated: 2026-09-23T23:01:28Z
---

## Description
XARA-US-0060 carried render TODO 5: "render the `.xar` corpus vs the original at 25/100/400 %". Our side exists (`xarast-cli render --zoom`), but the original cannot be run here: its rasteriser is the closed 2006-era `libCDraw.a` (x86/ppc, wxGTK2), so no reference renders of the original exist.

The only renders *by the original* we have are the preview bitmaps it embedded in the files themselves (`.xar` preview records, tags 60–64, research/01). Proposal: extract each file's preview, export our render at the preview's pixel size over the paper, and compare with the same mean |Δ| measure `cargo xtask export-check` uses; record per-file numbers and outliers. The 25/100/400 % comparison needs the original running (an x86 VM with the 2006 binaries, as render TODO 3 does) — keep that as a separate, VM-gated step.

## Acceptance Criteria
- A tool (xtask or `xarast-cli`) that compares our render with each corpus file's embedded preview, and the per-file numbers in `docs/memory/render.md`.
- The zoom-level comparison against a running original is either done on a VM or explicitly deferred with the reason.

## Notes
Clean room: previews are the files' own content, used as test oracles locally; never copied into the repository.
