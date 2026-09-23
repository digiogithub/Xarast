---
id: XARA-T-0050
type: task
title: Present the canvas through GpuTileCache in xarast-shell / xarast-app (pan and Draft zoom without full-frame uploads)
status: done
parent: XARA-US-0011
author: mcp
labels: [render, gpu, ui]
created: 2026-09-23T12:24:47Z
updated: 2026-09-23T13:24:30Z
started: 2026-09-23T12:49:22Z
closed: 2026-09-23T13:24:30Z
---

## Description
XARA-US-0011 decided (docs/memory/render.md, "The GPU decision"): rasterise on the CPU, keep the pixels on the GPU as tiles, let the GPU move them. `xarast-render` now has the primitive behind the `gpu` feature: `GpuTileCache` (upload a rect of a CPU `Surface` into a tile at a texel offset, LRU, `encode` a composite into an `Rgba8Unorm` target on the caller's encoder), `TileGrid` (keys, `placement` for an axis-aligned level→view mapping) and `compose_cpu` (the byte-identical CPU reference / software tier).

Wire it in (owner of xarast-shell / xarast-app):
1. The shell owns one `GpuTileCache` on its device (default 256² tiles, capacity 128 = 32 MiB) and composites into the canvas texture instead of `Painter::set_canvas`'s full-frame `write_texture`.
2. The render thread reports, with each frame, the level (e.g. `cache::scale_step`) and the level-space rectangle its pixels cover; the shell uploads only the dirty sub-rectangles, cut at tile-grid lines with texel offsets (`upload(key, surface, rect, at)`).
3. On a pan or wheel zoom the shell re-composites immediately from resident tiles (`TileGrid::placement`), before the render thread answers; `Draft` zoom stops resampling on the CPU (`reuse::rescale`), and a zoom-out shows resident tiles in the border instead of the backdrop.
4. `Final` at rest stays a whole-viewport render (tiles rasterised one by one are not byte-identical to a whole frame: 105/120 corpus cases exact, see render.md), cut into tiles on upload.
5. Capability ladder (S4/U2.5): with no usable device keep today's CPU path; `compose_cpu` is the same operation on the CPU.
6. Invalidate the cache on scene epoch, background/page colour or level change.

## Acceptance Criteria
- A 1080p and a 4K pan on the Intel iGPU present without a full-frame upload (measured: 4K full upload 23–27 ms today vs 1.5–3.0 ms composite).
- `--screenshot` read-back of a panned frame equals the CPU path's frame.
- No change in `xarast-render` needed beyond wiring; report any API gap back to render.

## Notes
Numbers and the bench: `cargo bench -p xarast-render --features gpu --bench tiles`; parity: `cargo test -p xarast-render --features gpu --test parity_tiles`.
