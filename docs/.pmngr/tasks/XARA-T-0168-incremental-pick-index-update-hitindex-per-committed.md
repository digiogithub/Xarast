---
id: XARA-T-0168
type: task
title: "Incremental pick index: update HitIndex per committed transaction instead of a lazy full rebuild (62 ms at 100k)"
status: done
parent: XARA-US-0031
author: mcp
labels: [phase-7, app-core, perf]
created: 2026-09-23T18:11:06Z
updated: 2026-09-23T19:28:20Z
started: 2026-09-23T18:17:47Z
closed: 2026-09-23T19:28:20Z
---

## Description
`xarast-app/src/picking.rs` rebuilds the whole index on the first pick after any change (62 ms at 100 000 objects, `cargo bench -p xarast-app --bench pick`). Apply set_bounds/insert/remove/set_z from the committed transaction's touched nodes, per the contract in `docs/memory/geometry.md`. Also image-alpha picking.

## Acceptance Criteria
First click after a one-object edit at 100k objects ≤ 2 ms; undo stays ≤ 1 ms.
