---
id: XARA-T-0292
type: task
title: "Bitmap gallery leftovers: F11, Ctrl-drop as page background, rename, file dropped on an object as a fill, live desktop check"
status: backlog
priority: low
parent: XARA-US-0055
author: mcp
labels: [phase-10, ui]
created: 2026-09-24T11:52:16Z
updated: 2026-09-24T11:52:16Z
---

## Description
Left over from XARA-US-0055:
- F11 does not show or focus the bitmap gallery pane (same missing dock plumbing as XARA-T-0254 for F9).
- The original's Ctrl-drop of a gallery bitmap on empty canvas sets the page background (`Kernel/sgbitmap.cpp:517-583`, facts); not done.
- The original's "inside" versus a group when dropping on a compound object; we drop on the leaf.
- Rename in the gallery (needs an undoable resource-name action).
- A dropped *file* over an object still always places a new bitmap; the original's file drop onto an object may fill it — research first.
- Not seen in a real window: the Import chooser, the progress bar, and which clipboard flavour Nautilus/Dolphin expose for a copied file (the shell reads `text/plain` only; if a manager offers only `text/uri-list` or `x-special/gnome-copied-files`, a data-control reader is needed). Manual check list in `ui.md` Open TODOs.

## Acceptance Criteria
- Each item done or explicitly deferred with a reason in `tools.md`/`ui.md`.
