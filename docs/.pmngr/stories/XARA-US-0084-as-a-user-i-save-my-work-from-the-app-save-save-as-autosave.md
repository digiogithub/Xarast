---
id: XARA-US-0084
type: story
title: As a user, I save my work from the app (Save, Save As, autosave, recovery)
status: in_review
priority: critical
parent: XARA-EP-0007
author: mcp
labels: [phase-6, app-core, ui]
created: 2026-09-23T18:43:30Z
updated: 2026-09-23T19:55:34Z
started: 2026-09-23T18:44:41Z
---

## Description
The `.xarast` format reads and writes exactly (corpus round trip 59/59), but the app has no File › Save. Wire the format into the application.

## Acceptance Criteria
- File › Save (Ctrl+S), Save As… (Ctrl+Shift+S) through the portal save dialog; saving a `.xar` asks for a `.xarast` name (no `.xar` writer).
- Dirty marker in the title and a confirm-on-close/quit dialog for unsaved changes.
- Document lock on open (read-only / open a copy / force UX, T-0086), released on close and on SIGINT/SIGTERM.
- Autosave + journal + recovery on next start (T-0087) — at least autosave and recovery offer.
- Thumbnail written on save through the renderer (T-0082).
- Save runs off the UI thread; ProbeX16 save ≤ 1 s budget holds.
