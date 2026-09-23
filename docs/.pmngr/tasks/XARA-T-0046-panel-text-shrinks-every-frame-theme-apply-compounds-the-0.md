---
id: XARA-T-0046
type: task
title: "Panel text shrinks every frame: theme::apply compounds the 0.92 font scale"
status: todo
priority: high
author: mcp
labels: [ui, defect]
created: 2026-09-23T12:09:55Z
updated: 2026-09-23T12:09:55Z
---

## Description
Found while measuring fractional scaling on GNOME 46 (XARA-US-0012). `xarast_ui::theme::apply` starts from `(*ctx.style()).clone()` and multiplies every text style size by 0.92 (rounded), and `Workspace::ui` calls it every frame. The shrink compounds: 12.5 → 12 → 11 → … → 6, where rounding makes it stable. After a few frames all panel text is ~6 pt (12 px at 2×) while widget boxes keep their size. Visible in any live window; a first-frame `--screenshot` still shows larger text, which is how it went unnoticed.

## Acceptance Criteria
- Text sizes are derived from a fixed base (egui's default style, or a stored base), not from the previous frame's style.
- A test runs `apply` twice (or N frames) and asserts the body size is unchanged.

## Notes
Crate `xarast-ui` (not owned by the shell workstream this round).
