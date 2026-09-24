---
id: XARA-T-0224
type: task
title: T9.4.7–T9.4.8 — IME preedit and caret area; text clipboard
status: done
parent: XARA-US-0047
author: mcp
labels: [phase-9, tools, text]
created: 2026-09-23T21:28:57Z
updated: 2026-09-24T09:32:20Z
started: 2026-09-24T07:28:19Z
closed: 2026-09-24T09:32:20Z
---

## Description
Feed the IME caret area from the text caret (`OverlayShape::Caret` geometry) and display preedit in the story; paste as text and copy with attributes inside Xarast while a caret is up (Ctrl+C/X/V currently act on objects).

## Acceptance Criteria
- IME enabled exactly while a text caret is up; candidate window under the caret on GNOME and one wlroots compositor.
- Ctrl+C/X/V with a text selection copy/cut/paste text, not objects.

## Notes
Depends on T9.4.6.
