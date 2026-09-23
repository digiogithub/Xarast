---
id: XARA-US-0031
type: story
title: W3 — Selection and precise hit testing
status: done
priority: high
parent: XARA-EP-0008
author: mcp
labels: [phase-7, app-core, geometry]
created: 2026-09-23T09:42:03Z
updated: 2026-09-23T18:13:58Z
started: 2026-09-23T16:44:05Z
closed: 2026-09-23T18:13:58Z
---

## Description
As a user, clicking selects the object actually under the cursor, not its bounding box.

## Tasks (full table: phase-07 §W3)
- T3.1 Hit testing on real geometry (`hit_fill`/`hit_stroke`, image alpha), z-order, layer lock/visibility. Replaces today's bbox `select_in_rect`.
- T3.2 Group traversal (Ctrl-click leaf, Alt-click beneath).
- T3.3 Marquee touch vs enclose.
- T3.4 Shift/toggle, Ctrl+A, Esc.
