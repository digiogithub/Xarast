---
id: XARA-T-0295
type: task
title: "Text tool: remove a story holding only paragraph breaks when editing ends"
status: in_review
priority: medium
parent: XARA-US-0047
author: mcp
labels: [phase-9, tools, text]
created: 2026-09-24T12:07:00Z
updated: 2026-09-24T12:41:20Z
started: 2026-09-24T12:30:07Z
---

## Description
Maintainer decision (2026-09-24), follow-up to XARA-T-0237: a story whose content is only paragraph breaks (no characters) is removed as well, including when editing ends (Esc, tool switch, click elsewhere), as the original does (`tools/texttool.cpp:1570`, `:2403`).

T-0237 deliberately kept tools invariant 11 ("leaving is never an edit"). This task revises that invariant for this one case.

## Acceptance Criteria
- Leaving the text tool with a story that holds only paragraph breaks removes it.
- The removal merges into the previous undo step (as the original merges its story-deletion op), or is otherwise justified; one undo restores the story exactly.
- A deletion that leaves only breaks also removes the story (extends T-0237's "only the final break" rule).
- Imported stories that are empty are never touched unless edited in this session.
- tools.md invariant 11 and decision 70 updated.
