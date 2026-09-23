---
id: XARA-US-0001
type: story
title: As a user, I open a .xar file in the running app and see it on the canvas
status: in_progress
priority: critical
parent: XARA-EP-0006
author: mcp
labels: [phase-5, app-core, shell]
estimate: 8
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T09:52:52Z
started: 2026-09-23T09:52:52Z
---

## Description
The last real piece of phase 5: wire `xarast-shell`, `xarast-ui` and `xarast-app` into one running application. Today they are correct separately but not connected. This is the first thing to do before phase 6.

## Acceptance Criteria
- `ShellEvent` is translated to `xarast_app::Intent` in `xarast-app` or a thin adapter (not in `input::translate`); physical `Modifiers`/`PointerButton` map to semantic ones.
- Render-thread protocol exists: `RenderRequest`, generations, cancellation (phase-05 §U5.5), one channel type.
- `xarast <file.xar>` opens and renders a corpus file; pan and zoom work.
- `xarast-cli` `render` and `smoke-open` subcommands wired to the headless API.

## Notes
See `docs/memory/ui.md` and `docs/memory/app-core.md` Open TODOs.
