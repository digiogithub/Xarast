---
id: XARA-T-0237
type: task
title: "Text tool: remove a story emptied by deletion (as the original does)"
status: done
priority: medium
parent: XARA-US-0047
author: mcp
labels: [phase-9, tools, text]
created: 2026-09-23T22:14:16Z
updated: 2026-09-24T12:37:57Z
started: 2026-09-24T11:08:03Z
closed: 2026-09-24T12:37:57Z
---

## Description
Since XARA-T-0223 a story whose text is deleted entirely stays in the document (one line holding only its final paragraph break). The original removes an empty story when the caret leaves it. Doing that here would add an extra undo step (or needs merging into the deletion's step). Decide with the maintainer and implement.

## Acceptance Criteria
- Leaving an emptied story either removes it (and undo restores it in one step with the text) or the choice to keep it is recorded in docs/memory/text.md.

## Notes
See docs/memory/text.md "Typing and deleting" and the Open TODOs there; tools.md decision 57.
