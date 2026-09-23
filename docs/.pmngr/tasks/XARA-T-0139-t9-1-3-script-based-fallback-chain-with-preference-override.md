---
id: XARA-T-0139
type: task
title: T9.1.3 — Script-based fallback chain with preference override
status: done
parent: XARA-US-0044
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:20Z
updated: 2026-09-23T17:24:20Z
---

## Description
`fallback_for(c, base)`: the script's preferred families (ICU4X Script property), the platform fallback from fontique, then the generic families, each checked against the face's charmap. `set_fallback_preference(ScriptTag, families)` overrides the chain per script; `set_generic_families` configures isolated databases. The shaper gets the same fallback through parley.

## Acceptance Criteria
- Hebrew/Arabic/Han/Hiragana resolve to their pinned faces; the override changes the result; uncovered characters return None (`tests/fontdb.rs`).

## Notes
Commit d441e92.
