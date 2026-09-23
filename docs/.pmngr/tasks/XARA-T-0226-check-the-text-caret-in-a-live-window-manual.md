---
id: XARA-T-0226
type: task
title: Check the text caret in a live window (manual)
status: backlog
parent: XARA-US-0047
author: mcp
labels: [phase-9, text, ui, manual]
created: 2026-09-23T21:28:57Z
updated: 2026-09-23T21:28:57Z
---

## Description
The caret, split caret, highlight, blink and key routing are covered headlessly only (no windows were launched). Verify by hand.

## Acceptance Criteria
- F8, click a story of TextDesigns/hebrew.xar → blinking caret; Right walks the Hebrew right-to-left on screen; Shift+arrows highlight; blink stops after 10 s idle; arrows do not pan while the caret is up; Esc leaves the text.

## Notes
Steps in docs/memory/ui.md Open TODOs.
