---
id: XARA-T-0044
type: task
title: "E1/E4: run on real compositors and check CSD on GNOME"
status: done
parent: XARA-US-0012
author: mcp
labels: [shell, experiment]
created: 2026-09-23T12:09:55Z
updated: 2026-09-23T12:09:55Z
---

## Description
E1 (build and run on GNOME/mutter, KDE/kwin, sway) and E4 (CSD on GNOME via sctk-adwaita) for the pinned winit 0.30.13.

## Notes
- GNOME 46 / mutter 46.2, isolated `gnome-shell --headless --wayland` with a virtual monitor on a private bus (Vulkan, NVIDIA RTX 4000 SFF Ada): app and probe run; 59-corpus-capable app opens BLUECAR.xar; pointer, keyboard, focus, resize (maximise 1600×933, unmaximise, 60-step live border drag with no validation error), minimise (no 0×0 on Wayland, loop keeps answering, ~0 CPU while hidden, restore redraws).
- E4: CSD drawn by sctk-adwaita with title and close button; the button layout follows the settings portal (close only under GNOME defaults, minimise/maximise/close with no portal).
- COSMIC: the live desktop, already measured in XARA-US-0001 (59/59 corpus). Not re-run this round to keep off the maintainer's desktop.
- KDE/kwin and sway: not installed on this machine — unmeasured.
