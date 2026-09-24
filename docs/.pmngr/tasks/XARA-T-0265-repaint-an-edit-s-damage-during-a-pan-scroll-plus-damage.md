---
id: XARA-T-0265
type: task
title: Repaint an edit's damage during a pan (scroll plus damage)
status: backlog
parent: XARA-US-0040
author: mcp
labels: [phase-8, render, perf]
created: 2026-09-24T02:18:01Z
updated: 2026-09-24T02:18:01Z
---

## Description
Follow-up to XARA-T-0221. `reuse::repaint` only repaints an edit's damage when the job's transform is bit-identical to the kept frame's. A new scene that arrives together with a whole-pixel pan (for example an edit made while auto-scrolling, or a drag that pans and previews in the same frame) is drawn as a full frame. It could scroll the kept pixels, rasterise the exposed strips, and repaint the damage translated by the scroll.

Also: a Draft frame that scrolled marks the whole viewport as inexact (conservative), so the Final after a Draft pan is always a full frame. Tracking the Draft strips instead would let that Final repaint only them.

## Acceptance Criteria
- An edit plus a whole-pixel pan produces `FrameReuse::Scrolled` or `Repainted` with strips ∪ translated damage as `fresh`, and the byte-exact corpus test in `xarast-shell/tests/edit_damage.rs` covers it.
- The tile planner still uploads only `fresh`.
