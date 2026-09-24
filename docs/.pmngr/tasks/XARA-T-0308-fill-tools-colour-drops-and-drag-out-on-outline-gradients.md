---
id: XARA-T-0308
type: task
title: "Fill tools: colour drops and drag-out on outline gradients, minor-axis follow, document nudge size"
status: backlog
parent: XARA-US-0039
author: mcp
labels: [phase-8, tools]
created: 2026-09-24T17:46:03Z
updated: 2026-09-24T17:46:03Z
---

## Description
Gaps left by XARA-T-0220 (tools.md decisions 76–79):
- The colour bar and colour editor resolve interior handle sets only (`colour_bar::selected_stop` returns `None` for an outline handle; `resolve_canvas` uses `fill_sets`). A colour dropped on an outline stop or arm should edit that outline stop/insert a stop.
- Drag-out only makes interior fills; there is no way to create an outline gradient from the tool (the original has none either — decide and document).
- The original turns an elliptical radial fill's minor axis to stay perpendicular (same length) when the major axis is dragged without Adjust (`Kernel/fillattr.cpp:7249-7310`); ours leaves it where it was.
- The keyboard nudge uses a provisional 1 mm unit (`NUDGE_UNIT_MP`); the document's own nudge size (`TAG_DOCUMENTNUDGE`, 4114) is imported and dropped.

## Acceptance Criteria
- Each item has a test through `Session::apply` in `crates/xarast-app/tests/fill_tool.rs` or `colour_bar.rs`; every gesture one undo step with exact undo.
