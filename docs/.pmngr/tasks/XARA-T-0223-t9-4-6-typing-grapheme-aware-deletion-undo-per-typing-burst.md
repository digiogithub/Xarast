---
id: XARA-T-0223
type: task
title: T9.4.6 — Typing, grapheme-aware deletion, undo per typing burst
status: in_progress
parent: XARA-US-0047
author: mcp
labels: [phase-9, tools, text]
created: 2026-09-23T21:28:57Z
updated: 2026-09-23T21:48:04Z
started: 2026-09-23T21:48:04Z
---

## Description
The text tool (XARA-US-0047) places carets and selections but does not type. Add InsertText/DeleteRange commands through the bus, create the story from the tool's `TextEditing::Pending { at, column }` state on the first character, give Delete/Backspace/Enter meaning (they are swallowed today), and route typed characters from the shell (plain character keys are already kept from firing shortcuts while a caret is up).

## Acceptance Criteria
- Typing 200 characters is one undo step (phase-09 criterion 14), CoalesceKey = story, 500 ms burst.
- Backspace/Delete remove whole grapheme clusters; caret stays valid after reflow.
- A pending point story or column becomes a real story on the first character; no empty story is left otherwise.

## Notes
See docs/memory/text.md "The text tool" and tools.md decisions 45–48.
