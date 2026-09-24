---
id: XARA-T-0268
type: task
title: T9.4.7 follow-up — draw an IME composition at a pending text caret
status: backlog
parent: XARA-US-0047
author: mcp
labels: [phase-9, text, tools]
created: 2026-09-24T07:50:52Z
updated: 2026-09-24T07:50:52Z
---

## Description
A composition in an existing story is drawn through `Preview::text` (the walker splices it into the story). At a pending caret (click on empty canvas, no story yet) there is no story to splice into, so the preedit is not shown; only the candidate window follows the caret and the commit creates the story (XARA-T-0224).

## Acceptance Criteria
- A composition at a pending caret is drawn at the caret with the current attributes (e.g. a `Preview` phantom story laid out by the walker), without creating a node or an undo step.
- Cancel leaves nothing; commit creates the story as today.
