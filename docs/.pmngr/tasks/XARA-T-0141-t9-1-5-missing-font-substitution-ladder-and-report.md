---
id: XARA-T-0141
type: task
title: T9.1.5 — Missing-font substitution ladder and report
status: done
parent: XARA-US-0044
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:20Z
updated: 2026-09-23T17:24:20Z
---

## Description
The five-rung ladder of the phase document: exact (case/whitespace-insensitive), style suffix stripped, metric-compatible aliases (table written from public facts, contents in docs/memory/text.md), generic family from PANOSE or the name, last resort. `FontSubstitution { requested, used, reason }` recorded, never written back.

## Acceptance Criteria
- Each rung tested (`tests/fontdb.rs`, `font/substitute.rs` unit tests).

## Notes
Not assigned this round but implemented with the query path. Commit d441e92.
