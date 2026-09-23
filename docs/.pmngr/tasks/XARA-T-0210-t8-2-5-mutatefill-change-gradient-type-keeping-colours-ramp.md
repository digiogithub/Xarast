---
id: XARA-T-0210
type: task
title: "T8.2.5 — MutateFill: change gradient type keeping colours, ramp and a mapped set of control points"
status: backlog
parent: XARA-US-0038
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:53:36Z
updated: 2026-09-23T19:53:36Z
---

## Description
Not in the W8.1/W8.2 model round. Implement `mutate_fill(g, to, obj_bounds) -> (FillGeometry, MutationLoss)` and the `MutateFill` command in `crates/xarast-doc/src/fill_edit.rs` following the mapping table in phase-08 §W8.2 (any→Flat takes `from`; Flat→gradient uses the object's bbox; Linear↔Diamond↔Radial `centre := start, major := end, minor := start + perp`; to/from three/four colour drops the ramp and seeds the extra colours). Reuse `set_own_attr`, `rebuild_ramp`, `stop_value`.

## Acceptance Criteria
- A test for all 56 shape pairs; `MutationLoss::ramp_dropped` reported; digest-exact undo.
