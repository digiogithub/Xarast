---
id: XARA-T-0059
type: task
title: Empty state with an Open… button and the recent files list
status: in_progress
priority: high
parent: XARA-US-0082
author: mcp
labels: [phase-5, ui]
created: 2026-09-23T13:50:18Z
updated: 2026-09-23T13:50:18Z
started: 2026-09-23T13:50:18Z
---

## Description
Launching with no argument shows, in the canvas area, an empty state with an "Open…" button, the recent files and a drop hint, instead of a blank canvas.

## Acceptance Criteria
- Headless test: the button raises the open intent; a recent entry raises an open of that path.
- Screenshot of a real window with no file.
