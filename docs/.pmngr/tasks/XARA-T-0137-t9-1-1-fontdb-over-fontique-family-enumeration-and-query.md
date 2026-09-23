---
id: XARA-T-0137
type: task
title: "T9.1.1 — FontDb over fontique: family enumeration and query"
status: done
parent: XARA-US-0044
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:20Z
updated: 2026-09-23T17:24:20Z
---

## Description
`FontDb` wraps a fontique collection behind a mutex (`&self` API, `Arc`-shareable); no fontique type is public. `families()` sorted/deduplicated; `query(FontQuery { family, weight, style, stretch })` with CSS matching and synthesis; `new_system()` defers enumeration to `load_system_fonts()`; `new_isolated()` for deterministic tests. fontconfig is dlopen'ed.

## Acceptance Criteria
- Weight/style matching tested on the pinned Noto set (`tests/fontdb.rs`).
- Opt-in system test: 2 202 families enumerated in ~40 ms (budget 300 ms).

## Notes
Commit d441e92 (deps 575bb41, fixtures de66c83, tests f7c05dd).
