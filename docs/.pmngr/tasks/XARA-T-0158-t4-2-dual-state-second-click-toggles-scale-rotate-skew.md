---
id: XARA-T-0158
type: task
title: "T4.2 Dual state: second click toggles scale ⇄ rotate/skew handles; draggable rotation centre with its own snap"
status: done
parent: XARA-US-0032
author: mcp
labels: [phase-7, tools]
created: 2026-09-23T18:10:41Z
updated: 2026-09-23T18:10:41Z
---

## Description
Selector dual state (`xarast-app/src/selector.rs`), keyed by the selection; rotation centre drag snaps to the box's nine anchor points and is carried by move/scale.

## Notes
Commits 69d674e (feature), 89e808b (commands). Test `dual_state_click_sequence`, `the_rotation_centre_drags_snaps_and_is_rotated_about` in `crates/xarast-app/tests/transforms.rs`.
