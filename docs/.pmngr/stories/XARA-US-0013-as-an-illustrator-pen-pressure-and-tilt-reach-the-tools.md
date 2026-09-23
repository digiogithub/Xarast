---
id: XARA-US-0013
type: story
title: As an illustrator, pen pressure and tilt reach the tools
status: backlog
parent: XARA-EP-0017
author: mcp
labels: [shell, tablet, hardware]
created: 2026-09-23T09:40:31Z
updated: 2026-09-23T09:40:31Z
---

## Description
Phase risk K6. Today there is no pen pressure (winit 0.30), no Linux trackpad gestures, and DnD gives no drop position. The axis pipeline is written and tested against scripted axes.

## Acceptance Criteria
- Hardware tablet validated; `uinput` virtual tablet in CI.
- Decision recorded: vendor+patch `octotablet` (with `dlopen`), or move to winit 0.31 once `accesskit_winit` supports it.

## Notes
X11 pressure is a documented degradation, not a target.
