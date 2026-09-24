---
id: XARA-T-0284
type: task
title: "Bitmap fill handles: perspective behaviour of the original and the outline (stroke) slot"
status: backlog
priority: low
parent: XARA-US-0052
author: mcp
labels: [phase-10, bitmap, tools]
created: 2026-09-24T10:08:48Z
updated: 2026-09-24T10:08:48Z
---

## Description
Left open by XARA-T-0271. Bitmap fill handles follow the original's virtual points (centre, middles of the axis edges) for plain fills (`tools.md` decisions 64–65). For a fill **in perspective** the tool shows four free corners plus the quadrilateral's centre — our choice; the original's perspective bitmap handles were not researched. Handles for bitmap fills in the **outline** slot are not shown (the phase-8 limitation of all fill handles).

## Acceptance Criteria
- Research the original's perspective bitmap fill handles (facts only, `file:line`) and match or record the deviation.
- Stroke-slot bitmap handles, if the fill tools gain stroke-slot handles at all.
