---
id: XARA-T-0055
type: task
title: "E2/E6/E7: tablet axes, trackpad gestures and soak on winit 0.30"
status: done
parent: XARA-US-0012
author: mcp
labels: [shell, experiment]
created: 2026-09-23T12:40:34Z
updated: 2026-09-23T12:40:34Z
---

## Notes
- E2: winit 0.30 has no tablet API, so pressure/tilt/twist cannot arrive (by construction). A tablet exists on this machine: `Wacom USB Bamboo PAD Pen` (/dev/input/event21; ABS X, Y, PRESSURE — no tilt) plus its finger pad. No `uinput` virtual tablet was created: the live session's libinput would adopt it. Handed to XARA-US-0013.
- E6: mutter 46 advertises `zwp_pointer_gestures_v1` v3, but winit 0.30's Linux backends contain no gesture code; mutter's RemoteDesktop API cannot inject touchpad gestures. Pinch-zoom remains Ctrl+wheel.
- E7 (shortened): 30 min on GNOME 46, 1,663 rounds of synthetic input on BLUECAR.xar; RSS 199–247 MB, flat at 202.6 MB for the last 10 min; 40 threads and 77 fds constant; no error or protocol error. The 8-hour run is still owed.
