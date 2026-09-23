---
id: XARA-T-0159
type: task
title: "T4.3 Scale gesture: corner and edge blobs, aspect with Ctrl, from centre with Shift, optional line-width scaling"
status: done
parent: XARA-US-0032
author: mcp
labels: [phase-7, tools]
created: 2026-09-23T18:10:41Z
updated: 2026-09-23T18:10:41Z
---

## Notes
Commits 89e808b (TransformNodes honours scale_line_widths by √|det|; labels), 69d674e (scale drags). Tests: `a_corner_drag_scales_about_the_opposite_corner_in_one_step`, `constrain_keeps_the_aspect_and_adjust_scales_about_the_centre`, `constrain_pressed_mid_scale_changes_the_result_without_moving`, `scaling_scales_line_widths_unless_turned_off`.
