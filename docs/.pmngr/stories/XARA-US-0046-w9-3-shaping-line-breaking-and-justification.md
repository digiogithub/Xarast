---
id: XARA-US-0046
type: story
title: W9.3 — Shaping, line breaking and justification
status: backlog
parent: XARA-EP-0010
author: mcp
labels: [phase-9, text]
created: 2026-09-23T09:42:40Z
updated: 2026-09-23T09:42:40Z
---

## Description
As a user, imported text renders (13 of 21 blank corpus files are blank because of text).

## Tasks (full table: phase-09 §W9.3)
- T9.3.1 `Shaper` façade over `parley`.
- T9.3.2 `FontMetrics` for the doc layer.
- T9.3.3 Line breaking via ICU4X segmentation.
- T9.3.4 Justification with char/space slack.
- Scene walker draws text (`WalkStats::text_pending` → 0).
