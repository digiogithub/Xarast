---
id: XARA-T-0264
type: task
title: "Text infobar leftovers: script, baseline and aspect controls, spacing mode, line-start style, ruler edge cases"
status: backlog
parent: XARA-US-0047
author: mcp
labels: [phase-9, tools, text, ui]
created: 2026-09-24T01:52:50Z
updated: 2026-09-24T01:52:50Z
---

## Description
Gaps left by XARA-T-0225 (text infobar, OpenType panel, text ruler):
- The bar has no controls for super/subscript, baseline shift or aspect ratio (the model and `SetTextAttr` already handle them).
- Line spacing cannot be switched between ratio (%) and absolute (pt) from the bar; the field reads the number in the paragraph's current mode.
- Text typed at the start of a line takes the line's attribute scope, not the following character's own attributes, while the bar shows the following character's values (`insert_text` copies only the previous character's own children).
- Removing every tab stop from the ruler falls back to the ruler a `.xar` `TextLine` node carries, so imported stops can reappear.
- The ruler is not shown for turned, sheared or mirrored stories, nor for text on a path.
- The font chooser is a plain name list (no previews; T9.1.7's gallery).

## Acceptance Criteria
- Each gap either fixed with a test or recorded as a decision in `docs/memory/text.md` / `tools.md`.
