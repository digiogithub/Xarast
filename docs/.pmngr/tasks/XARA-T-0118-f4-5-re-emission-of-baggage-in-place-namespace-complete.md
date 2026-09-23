---
id: XARA-T-0118
type: task
title: F4.5 Re-emission of baggage in place, namespace-complete, byte-stable
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
The writer's re-emission (W3) plus the reader half: fragments are captured as the exact text read, completed with only the inherited declarations they use (not the document's default SVG namespace), so the first re-save is a fixed point; positions put them back inside the element they were read in.

## Notes
Done in 967da9a (`dom.rs::fragment`); fixed point asserted in `tests/svg_read.rs` and `fuzz_xarast_svg_read`.
