---
id: XARA-US-0012
type: story
title: As a user on GNOME/KDE Wayland, portals, clipboard and DnD work for real
status: in_review
priority: high
parent: XARA-EP-0017
author: mcp
labels: [shell, hardware]
created: 2026-09-23T09:40:31Z
updated: 2026-09-23T12:40:56Z
started: 2026-09-23T11:45:54Z
---

## Description
Run experiments E1–E7 from phase 5 on a machine with a compositor and session bus. The winit verdict and all of W3 are provisional until then.

## Acceptance Criteria
- E1–E7 results recorded in `docs/memory/ui.md`.
- File dialogs via portal, clipboard round-trip (text + image), drag-and-drop in and out verified on at least two compositors.
