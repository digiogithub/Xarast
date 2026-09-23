---
id: XARA-T-0045
type: task
title: "E3: fractional scaling at 1.25 / 1.5 / 2.0, IME caret and cursor shapes"
status: done
parent: XARA-US-0012
author: mcp
labels: [shell, experiment]
created: 2026-09-23T12:09:55Z
updated: 2026-09-23T12:09:55Z
---

## Notes
Isolated GNOME 46, scale set through org.gnome.Mutter.DisplayConfig (scale-monitor-framebuffer):
- wp_fractional_scale_v1 preferred_scale 150/180/240 → ScaleChanged 1.25/1.5/2.0 followed by Resized with physical = round(logical × scale) (800×500 → 1000×625 at 1.25). Screenshots crisp at 1.5 and 2.0.
- GNOME exposes 1.5038 (not 1.5) so logical sizes stay integral; the protocol carries 1.5 (120ths), so the compositor resamples by 0.25 %. Inherent to fractional-scale-v1 on mutter, not ours.
- IME (ibus, cangjie3 in the nested session): preedit/enable/disable events arrive; the candidate popup opens directly under the caret area at 1× and at 1.25× with the area given in device pixels.
- Cursor shapes: all 12 tried (Default, Text, Hand, Grab, Grabbing, Move, Crosshair, NotAllowed, ResizeHorizontal, ZoomIn, Wait, Hidden) show the right themed cursor (mutter 46 has no cursor-shape-v1; winit uses the theme).
- Found while doing this: panel text shrinks to ~6 pt after a few frames at any scale — see the xarast-ui task.
