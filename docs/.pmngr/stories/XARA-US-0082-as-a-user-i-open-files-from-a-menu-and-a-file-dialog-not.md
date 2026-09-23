---
id: XARA-US-0082
type: story
title: As a user, I open files from a menu and a file dialog, not only from the command line
status: done
priority: critical
parent: XARA-EP-0006
author: mcp
labels: [phase-5, ui, shell]
created: 2026-09-23T13:46:42Z
updated: 2026-09-23T16:37:31Z
started: 2026-09-23T13:50:01Z
closed: 2026-09-23T16:37:31Z
---

## Description
Maintainer feedback (2026-09-23): the app has no menu; a file can only be opened as a CLI argument. Rendering and zoom work well.

## Acceptance Criteria
- Menu bar: File (Open… Ctrl+O, Open Recent, Close Ctrl+W, Quit Ctrl+Q), View (Zoom in/out, Fit page, Fit drawing, 100 %), Help (About, with third-party licence note placeholder).
- File › Open uses the XDG portal FileChooser (`PortalService`, ashpd), filtered to `.xar` (and all files), attached to the Xarast window if possible (T-0048).
- Launching with no argument shows an empty state with an "Open…" button and recent files, instead of a blank canvas.
- Opening replaces the current document (single-document for now); errors are shown, never panic.
- Recent files persisted under `$XDG_STATE_HOME/xarast/`.
- Drag-and-drop of a `.xar` onto the window opens it (already delivered by the shell).
- Keyboard shortcuts work and do not fire while a text field has focus.
