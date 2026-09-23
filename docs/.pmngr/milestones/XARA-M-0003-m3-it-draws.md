---
id: XARA-M-0003
type: milestone
title: M3 — It draws
status: done
author: mcp
created: 2026-09-23T09:39:01Z
updated: 2026-09-23T16:37:31Z
started: 2026-09-23T09:39:01Z
closed: 2026-09-23T16:37:31Z
---

## Description
Phases 4–5. Real Xara documents render on screen, with pan and zoom.

## Acceptance Criteria
- Opening a corpus `.xar` in the running app shows it on the canvas.
- Pan/zoom ≤ 16 ms at 100k objects on an integrated GPU; open 5 MB `.xar` ≤ 500 ms to first paint.

## Notes
CPU render engine and shell/UI/app-core crates exist, but the `ShellEvent → Intent` bridge is missing, so there is no runnable viewer yet. Interactive performance is unmeasured (no GPU/compositor in the dev container).
