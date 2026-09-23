---
id: XARA-T-0197
type: task
title: T8.1.3 — Cycle detection on reparent, cached resolve_order, PaletteEpoch, dirty propagation
status: done
parent: XARA-US-0037
author: mcp
labels: [phase-8, color]
created: 2026-09-23T19:52:05Z
updated: 2026-09-23T19:52:05Z
---

## Result
- Commits `d0b8d80` (table), `48807d8` (builder breaks loaded loops with `repair_cycles` + a `Repaired` warning).
- `reparent` refuses `Cycle` and `TooDeep`; `table::cycles` builds 1 000 random derivation graphs: no accepted cycle, no rejected acyclic edge, order always parents-first.
- `resolve_order` cached in a `OnceLock`, invalidated by structural changes; `PaletteEpoch` strictly increasing across every mutation, undo included (`advance_epoch_past`).
- Dirty propagation: `refresh_from` walks descendants in resolve order and reports the ids whose cached value changed.
