---
id: XARA-T-0238
type: task
title: T8.6.1 — ColourEditorModel with live/committed split
status: done
parent: XARA-US-0041
author: mcp
labels: [phase-8, app]
created: 2026-09-23T22:30:13Z
updated: 2026-09-23T23:20:36Z
started: 2026-09-23T22:30:13Z
closed: 2026-09-23T23:20:36Z
---

## Description
`xarast-app` model of the colour editor: target (selection fill/line, current attributes, palette entry), display model, derivation, live vs committed. A drag applies coalesced commands (one undo step); Esc restores the committed value and leaves the history unchanged.

## Acceptance Criteria
- A 60-event drag = one undo step; Esc mid-drag = digest and history unchanged.
- Projection `ColourEditorView` for the UI.
