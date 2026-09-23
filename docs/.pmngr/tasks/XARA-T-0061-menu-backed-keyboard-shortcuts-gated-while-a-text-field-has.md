---
id: XARA-T-0061
type: task
title: Menu-backed keyboard shortcuts, gated while a text field has focus
status: in_progress
priority: high
parent: XARA-US-0082
author: mcp
labels: [phase-5, shell]
created: 2026-09-23T13:50:18Z
updated: 2026-09-23T13:50:18Z
started: 2026-09-23T13:50:18Z
---

## Description
The temporary view keys (`+`/`-`/`1`/`0`/`d`, Home) become the shortcuts of menu commands, resolved through the shell's `ShortcutMap` over the `xarast-app` command table, plus Ctrl+O/W/Q. None fires while a text field has the keyboard (the existing `text_input` rule).

## Acceptance Criteria
- Headless tests: Ctrl+O raises the open request; typing into a rename field fires nothing.
