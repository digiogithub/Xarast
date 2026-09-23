---
id: XARA-T-0057
type: task
title: Menu bar (File, View, Help) drawn in-window by egui, backed by an app command table
status: done
priority: critical
parent: XARA-US-0082
author: mcp
labels: [phase-5, ui]
created: 2026-09-23T13:50:18Z
updated: 2026-09-23T14:14:13Z
started: 2026-09-23T13:50:18Z
closed: 2026-09-23T14:14:13Z
---

## Description
An in-window egui menu bar (no native/global menu: COSMIC and GNOME on Wayland have none). File: Open… (Ctrl+O), Open Recent ▸, Close (Ctrl+W), Quit (Ctrl+Q). View: Zoom in, Zoom out, Fit page, Fit drawing, 100 %. Help: About Xarast (version, `MIT OR Apache-2.0`, not affiliated with Xara Group Ltd, third-party licences placeholder). Items are backed by a command table in `xarast-app` and raise `Intent`s.

## Acceptance Criteria
- Visible at the first frame with and without a document, at scale 1 and 1.25.
- Menu items are labelled in the AccessKit tree.
- Headless egui test: menu click → intent.
