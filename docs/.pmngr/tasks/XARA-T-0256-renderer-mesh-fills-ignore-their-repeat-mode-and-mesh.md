---
id: XARA-T-0256
type: task
title: "Renderer: mesh fills ignore their repeat mode and mesh transparencies are flat means"
status: backlog
priority: medium
parent: XARA-US-0043
author: mcp
labels: [phase-8, render]
created: 2026-09-24T00:01:44Z
updated: 2026-09-24T00:01:44Z
---

## Description
Found while baking three- and four-colour fills into SVG (XARA-US-0043), which had to match the CPU renderer to pass `cargo xtask export-check`:

- `xarast-render` `PaintSampler::sample` clamps `(u, v)` to the unit square for `GradRamp::Mesh3`/`Mesh4` and never looks at the paint's `repeat`, although `xarast-app` `paint.rs::mesh_repeat` computes one (a mesh "clamps only when the mapping says do not repeat, and tiles otherwise — including the default", `research/01 §8.3`). `docs/memory/render.md` claims all six shapes × four repeat modes.
- Three- and four-colour **transparencies** are drawn as the flat mean of their levels (`xarast-app` `paint.rs`, "Meshes and procedurals have no transparency counterpart in the renderer").

The SVG profile (`xarast-format/src/svg/bake.rs`) mirrors the renderer today (clamped meshes, flat mesh transparencies); when this is fixed, the bake must follow (tile the rows' pattern; bake mesh transparencies as masks with `bake::mesh(…, Target::Mask)`).

## Acceptance Criteria
- Mesh fills honour `Repeat`/`Mirror`/`Simple` in the CPU sampler (and the GPU shader), with a golden image per mode.
- Mesh transparencies are evaluated per pixel.
- `svg/bake.rs` updated to match, `export-check` still within limits on the corpus.
