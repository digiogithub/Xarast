---
id: XARA-T-0043
type: task
title: "Clipboard: persistence claim was about focus loss; the real variable is exit"
status: done
priority: medium
parent: XARA-US-0012
author: mcp
labels: [shell, defect]
created: 2026-09-23T12:09:31Z
updated: 2026-09-23T12:09:31Z
---

## Description
Measured on GNOME 46 (with XWayland): text and image round trips work both ways between the probe (arboard, X11 fallback because mutter has no data-control) and a GTK peer, over Wayland and X11 peers; the RGBA image comes back byte-identical and unpremultiplied. A copy survives focus loss and even the probe exiting (mutter keeps the selection). Without XWayland (`--no-x11`) arboard falls back to `NullClipboard`: no clipboard at all.

## Acceptance Criteria
- `Clipboard::persists_after_exit` / `clipboard_survives_exit` replace the focus-loss claim; GNOME and X11 true, other Wayland false.

## Notes
Fixed in 1de398c. The GNOME-without-XWayland gap is filed separately.
