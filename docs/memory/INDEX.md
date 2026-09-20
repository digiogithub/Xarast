# Project memory index

Durable per-subsystem notes. Any agent working on a subsystem **must** read its
note first and update it afterwards.

| Note | Subsystem | Covers |
|---|---|---|
| `xar-import.md` | `.xar` importer | Tags implemented, format quirks, corpus files that still fail |
| `document-model.md` | Document model | Node arena, attributes, invariants, undo |
| `render.md` | Render engine | Pipeline, blend modes, caching, CPU/GPU decisions |
| `xarast-format.md` | Native format | Versioning, round-trip, SVG extensions |
| `ui.md` | User interface | Toolkit, panel layout, shortcuts, Wayland |
| `packaging.md` | Packaging | AppImage, CI, glibc compatibility |
| `text.md` | Text | Shaping, fonts, text on a path |
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
