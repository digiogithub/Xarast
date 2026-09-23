---
id: XARA-T-0196
type: task
title: "T8.1.2 — ColourTable editing: redefine, rename, reparent, remove, refresh_from"
status: done
parent: XARA-US-0037
author: mcp
labels: [phase-8, color]
created: 2026-09-23T19:52:05Z
updated: 2026-09-23T19:52:05Z
---

## Result
- Commit `d0b8d80` (`crates/xarast-color/src/table/edit.rs`).
- `redefine` returns the ids whose packed value moved; `rename` refuses duplicates; `reparent` is the only path to kind/parent (bakes the value when unlinking, starts a link with all components overriding, a tint/shade takes its parent's model — as the original); `remove(OnDelete::{Detach, Reject})`; `refresh_from`, `refresh_all`.
- Tints now per model (RGB/grey towards 1, CMYK scales inks, HSV scales S and lifts V); shades are signed HSV moves; the `.xar` reader keeps a shade's sign (`ColourKind::from_raw`).
- `redefine` on 256 entries with a 4-deep chain: 0.94 µs (budget 20 µs).
- Tests: `crates/xarast-color/tests/palette.rs`.
