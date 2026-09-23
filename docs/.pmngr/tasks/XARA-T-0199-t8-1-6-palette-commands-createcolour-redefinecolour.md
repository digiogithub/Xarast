---
id: XARA-T-0199
type: task
title: "T8.1.6 — Palette commands: CreateColour, RedefineColour, RenameColour, DeleteColour (OnDelete), ReparentColour"
status: done
parent: XARA-US-0037
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:52:05Z
updated: 2026-09-23T19:52:05Z
---

## Result
- Commit `48807d8`.
- Each is one `Action::SetPalette` (inverse = previous table), so undo restores a deleted entry under its old `ColourId`; `canonical_digest` now covers the palette and every command is proved digest-exact on undo and redo (`crates/xarast-doc/tests/fill_palette.rs`).
- `DeleteColour { policy: Detach }` rewrites every arena use (reachable or history-retained) to `Colour::Direct` of its resolved value, clears guide colours, detaches derived entries: the full-arena sweep finds zero dangling references (acceptance 12). `Reject` refuses with `EditError::Palette(StillReferenced)`.
