---
id: XARA-T-0140
type: task
title: T9.1.4 — Embedded-face registration shadowing system faces
status: done
parent: XARA-US-0044
author: mcp
labels: [phase-9, text]
created: 2026-09-23T17:24:20Z
updated: 2026-09-23T17:24:20Z
---

## Description
`register_embedded(name, bytes)` registers into fontique's own family map, which is consulted before the system map, so a registered family shadows an installed one of the same name. Unreadable bytes are rejected. `embedding_denied()` reads OS/2 fsType (restricted or bitmap-only).

## Acceptance Criteria
- Opt-in system test: an embedded face wins over a real installed bold face of the same family.
- fsType cases tested on patched fixtures.

## Notes
Commit d441e92.
