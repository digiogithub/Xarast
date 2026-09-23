---
id: XARA-T-0042
type: task
title: "Wayland drag and drop: winit 0.30 delivers no file drops at all"
status: done
priority: high
parent: XARA-US-0012
author: mcp
labels: [shell, defect]
created: 2026-09-23T12:09:31Z
updated: 2026-09-23T12:09:31Z
---

## Description
winit 0.30.13 implements HoveredFile/DroppedFile on X11 only. Measured on GNOME 46: a GTK3 drag source offering two `file:` URIs dropped on the window produced no event (the same drag onto a GTK window worked). The app's help text advertises drop-to-open.

## Acceptance Criteria
- Drops on Wayland arrive as `DragEvent::Entered/Moved/Dropped` with device-pixel positions and every file of a multi-file drop in one event.
- No change on X11 (winit's path) and no hard link to libwayland-client.

## Notes
Fixed in ea25248: `wayland_dnd.rs` binds `wl_data_device` (sctk 0.19, same versions winit links) on a second queue over winit's `wl_display`, dispatched by its own thread (dispatching from `about_to_wait` accepted the offer too late: the loop is parked while the compositor holds the pointer, and every drop came back cancelled). Evidence: probe got Entered at (27,152), 9 Moved, Dropped with both paths at the release point; the real app opened both dropped files; clean quit 146 ms.
DnD *out* (dragging from Xarast) is not possible with winit 0.30 and is not implemented.
