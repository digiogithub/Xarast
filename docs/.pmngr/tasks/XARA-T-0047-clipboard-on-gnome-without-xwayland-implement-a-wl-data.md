---
id: XARA-T-0047
type: task
title: "Clipboard on GNOME without XWayland: implement a wl_data_device selection fallback"
status: backlog
priority: low
parent: XARA-US-0012
author: mcp
labels: [shell]
created: 2026-09-23T12:09:55Z
updated: 2026-09-23T12:09:55Z
---

## Description
arboard uses wlr/ext-data-control where offered and otherwise X11. Mutter has no data-control, so on GNOME it goes through XWayland; with XWayland disabled (`--no-x11`) the shell gets `NullClipboard` (measured on GNOME 46). The Wayland DnD module (`wayland_dnd.rs`) already binds `wl_data_device` on winit's connection; the selection (read via the selection offer, write via a data source with a serial from our own wl_keyboard/wl_pointer) could reuse it.

## Acceptance Criteria
- Text (and PNG image) copy/paste work on GNOME with XWayland off.
