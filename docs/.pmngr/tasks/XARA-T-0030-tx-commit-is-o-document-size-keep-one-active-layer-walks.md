---
id: XARA-T-0030
type: task
title: "Tx::commit is O(document size): keep_one_active_layer walks the whole tree (1.04 ms per edit at 100k)"
status: backlog
priority: high
parent: XARA-US-0016
author: mcp
labels: [doc, perf, regression]
created: 2026-09-23T10:37:22Z
updated: 2026-09-23T10:37:22Z
---

Found by the reference-machine run (see `docs/memory/document-model.md`, `perf.md`). Introduced in 2005f67. Fix: spread index, or only check spreads whose layers the transaction touched. Bench: `dispatch/single_node_edit` must drop back to µs. Also `snapshot()` is 43–80 ms vs 25 ms budget.
