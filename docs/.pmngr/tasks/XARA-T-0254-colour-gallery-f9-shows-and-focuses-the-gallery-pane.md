---
id: XARA-T-0254
type: task
title: "Colour gallery: F9 shows and focuses the gallery pane"
status: backlog
parent: XARA-US-0042
author: mcp
labels: [phase-8, ui]
created: 2026-09-23T23:50:12Z
updated: 2026-09-23T23:50:12Z
---

## Description
The colour gallery exists as a dock pane (`xarast-ui/src/panels/gallery.rs`) but `F9` does nothing: `UiHost` has no API to show, re-dock or focus a pane, and there is no `AppCommand` for it.

## Acceptance Criteria
- `F9` (and a View menu item) shows the gallery if it was closed and focuses it.
- The binding is listed in the command table with its key.
