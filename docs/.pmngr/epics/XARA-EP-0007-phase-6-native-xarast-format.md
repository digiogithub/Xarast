---
id: XARA-EP-0007
type: epic
title: Phase 6 — Native .xarast format
status: in_progress
priority: high
milestone: XARA-M-0004
author: mcp
labels: [phase-6]
created: 2026-09-23T09:39:43Z
updated: 2026-09-23T13:50:59Z
started: 2026-09-23T13:50:59Z
---

## Description
`xarast-format`: ZIP container wrapping SVG plus deduplicated binary resources; opens with graceful degradation in browsers/Inkscape and full fidelity in Xarast. Atomic save, lock, autosave, journal, recovery.
Spec: `docs/phases/phase-06-xarast-format.md`, `docs/research/06-*`.

## Acceptance Criteria
- Round-trip of the corpus through `.xarast` without loss.
- Save a 20 MB `.xarast` ≤ 1 s.
