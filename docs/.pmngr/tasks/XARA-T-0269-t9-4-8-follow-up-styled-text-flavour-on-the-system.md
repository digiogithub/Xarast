---
id: XARA-T-0269
type: task
title: T9.4.8 follow-up — styled text flavour on the system clipboard; plain-text paste as a new story
status: backlog
parent: XARA-US-0047
author: mcp
labels: [phase-9, text, tools]
created: 2026-09-24T07:50:52Z
updated: 2026-09-24T07:50:52Z
---

## Description
XARA-T-0224 copies text with its attributes inside Xarast only: the system clipboard gets plain text (`arboard` offers text/HTML/images), and pasting styled text from other applications is plain. With no text caret up, plain text on the clipboard is read as SVG and refused.

## Acceptance Criteria
- Copying text also offers an HTML flavour (`arboard` `set_html`) with the character styles; pasting HTML from another application keeps bold/italic/size where it can.
- Paste with no caret up and plain (non-SVG) text on the clipboard creates a point story in the view centre (as the original's paste of text does), one undo step.
- Paragraph attributes on copy/paste decided with the maintainer (today the text takes the paragraph it lands in).
