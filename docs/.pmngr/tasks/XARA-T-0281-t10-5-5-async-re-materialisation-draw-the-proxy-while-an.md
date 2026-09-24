---
id: XARA-T-0281
type: task
title: "T10.5.5 Async re-materialisation: draw the proxy while an evicted base comes back"
status: backlog
parent: XARA-US-0053
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T09:54:30Z
updated: 2026-09-24T09:54:30Z
---

## Description
Gap left by XARA-US-0053. Today a sampler that needs an evicted level re-materialises it synchronously on the render thread (a spill read, ~1.2 ms per 16 MiB from the page cache, or a re-decode, tens to hundreds of ms). The Draft `Nearest` filter always samples the base, even when minified, so a zoomed-out Draft view of many evicted photos re-materialises every base.

## Acceptance Criteria
- The render thread never blocks on a spill read or decode: it draws from the resident proxy (Draft) and schedules the re-materialisation on a worker, then repaints the damage.
- Final/export renders stay byte-identical to an unlimited budget (tests in `xarast-render/tests/pixel_budget.rs` and `xarast-app/tests/pixel_budget.rs`).
- `build_scene` (fresh walker per call) stops re-decoding every bitmap: a per-document decoded-image cache.

## Notes
See docs/memory/render.md, "Pixel memory budget".
