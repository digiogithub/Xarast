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
| `export.md` | Export (`xarast-io`) | Export model and sizing, filter registry, raster encoders, PDF (crate spike, fidelity matrix, the ladder), SVG (the interchange dialect of the profile mapper), determinism contract, `Compromise` taxonomy, the `.xar` non-goal |
| `perf.md` | Performance | Benchmarks, budgets, regressions |

## Local gates = CI

The "Format, lint and test" job of `.github/workflows/ci.yml`, run locally.
Run all four, serially, before calling work done. Each one must exit 0.
The job sets `RUSTFLAGS=-D warnings` for every step, and it needs Poppler
(`pdftoppm`) or the PDF render comparisons skip. Leave `XARAST_GPU_TESTS`
unset: the GPU tests serialise on their own lock.

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`cargo deny check licenses bans sources advisories` is the `licences`
job; run it too when `Cargo.lock` changes.

The doc build is the one people forget: it failed on every push from
fc6eb54 (2026-09-23) to 8645d27 (2026-09-24) without anyone noticing.
Intra-doc links from a public item to a private one
(`rustdoc::private_intra_doc_links`) are errors under `-D warnings`.
Write a private name as a plain code span (`` `walker::ready_image` ``),
never as a link. Never allow the lint.
Where a module and a function share a name (`save`, `open`, `sniff` in
`xarast-format`), link with `mod@name` or `name()`. Add `--keep-going`
to see every crate's errors in one run.

The other per-push jobs are `export` and `reproducible` (the fixture
export, see `export.md`), `perf` (`xtask perf --tier pr`, see `perf.md`)
and `licences`. `fuzz.yml`, `export-corpus.yml` and `perf-nightly.yml` run on a schedule.
GitHub only schedules workflows that are on the default branch (`main`),
so none of them run until the branch is merged.

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
