---
id: XARA-T-0198
type: task
title: "T8.1.4 (doc side) — Colour::Indexed resolver API: PaletteResolver, ColourUses, palette epoch"
status: done
parent: XARA-US-0037
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:52:05Z
updated: 2026-09-23T19:52:05Z
---

## Result
- Commit `48807d8` (`crates/xarast-doc/src/palette.rs`).
- `PaletteResolver::resolve(&Colour, &ColourTable) -> Rgba8`, memoised per (id, local tint), cleared when the palette epoch changes; quantises as the original (`to_rgba8_packed`).
- `ColourUses` (`build`/`rebuild`/`users`/`users_of`): reverse index ColourId → owner nodes; 5 000-object test gets exactly the 5 000 users of a redefined parent's tint.
- `Document::palette_epoch()`, `palette::changed_between`, `palette_refs`.
- The walker wiring is a separate task (app crate).
