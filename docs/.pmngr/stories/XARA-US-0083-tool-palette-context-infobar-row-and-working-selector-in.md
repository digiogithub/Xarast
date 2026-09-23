---
id: XARA-US-0083
type: story
title: Tool palette, context infobar row and working selector in the viewer
status: in_review
priority: high
parent: XARA-EP-0008
author: mcp
labels: [phase-7, ui]
created: 2026-09-23T17:14:13Z
updated: 2026-09-23T17:14:23Z
started: 2026-09-23T17:14:19Z
---

## Description
The maintainer's request: a left-docked vertical tool palette in the egui workspace (Selector, Shape editor, Rectangle, Ellipse, Pen, Freehand, Zoom, Push; Fill/Transparency/Text greyed), per-tool keys via AppCommand, an infobar row under the menu, accessible labels; Selector click-select and move-drag working with undo in the real window.

## Acceptance Criteria
- Palette buttons switch tools; keys F2/F3/F4/Shift+F3/F4/F5/F7/F8 do the same.
- Unimplemented tools visibly "coming soon"; later-phase tools disabled.
- Click selects, drag moves (one "Move" undo step), Edit › Undo Move / Ctrl+Z restores.
- Headless egui tests and a real-window screenshot.
