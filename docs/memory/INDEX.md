# Project memory index

Durable per-subsystem notes. Any agent working on a subsystem **must** read its
note first and update it afterwards.

| Note | Subsystem | Covers |
|---|---|---|
| `geometry.md` | Geometry & colour | Millipoints, paths, booleans, colour models, palettes |
| `colour.md` | Colour editing (`xarast-color`, `xarast-doc` palette/fill commands) | Conversions vs. the original, palette editing and its DAG, epochs, fill/transparency commands, the corpus oracle |
| `xar-import.md` | `.xar` importer | Physical layer, tag coverage, format quirks, corpus acceptance numbers |
| `document-model.md` | Document model | Node arena, attributes, invariants, undo |
| `app-core.md` | Application core | `EditState`, `Viewport`, the arena→`Scene` walker, sessions, command dispatch, the shell/UI contract |
| `tools.md` | Editing & tools | Command bus in the app, tool machine, tools, preview, palette, infobar, undo labels |
| `render.md` | Render engine | Pipeline, blend modes, caching, CPU/GPU decisions |
| `xarast-format.md` | Native format | Versioning, round-trip, SVG extensions |
| `ui.md` | User interface | Toolkit, panel layout, shortcuts, Wayland |
| `packaging.md` | Packaging | AppImage, CI, glibc compatibility |
| `text.md` | Text | Shaping, fonts, text on a path |
| `image.md` | Images (`xarast-image`) | Decoding façade, decode limits and bomb fixtures, EXIF, `.xar` bitmap wrappings, the walker contract |
| `export.md` | Export (`xarast-io`) | Export model and sizing, filter registry, raster encoders, determinism contract, `Compromise` taxonomy, the `.xar` non-goal |
| `perf.md` | Performance | Benchmarks, budgets, regressions |

## Status

Notes are created as their phase starts. If one does not exist yet, create it
from this template:

```markdown
# <subsystem>

## Current state
## Decisions taken (and why)
## Invariants that must not be broken
## Dead ends (do not retry)
## Open TODOs
```
