---
id: XARA-T-0026
type: task
title: Give ViewTransform the viewport's Y orientation (vertical ruler reads negative)
status: backlog
priority: medium
parent: XARA-US-0081
author: mcp
labels: [ui]
created: 2026-09-23T10:31:45Z
updated: 2026-09-23T10:36:14Z
---

## Description
`xarast_ui::model::ViewTransform` is a scale plus an offset with y pointing down, and it has no flip. Document y points up, and only `xarast_app::Viewport` owns the flip. The composition root (`xarast_shell::viewer::document_view`) therefore passes the interface y-negated document coordinates. The page edge lands on the rendered page to the pixel (a test pins that), but the vertical ruler labels read negative.

## Acceptance Criteria
- The interface takes its orientation from the viewport, either by carrying a sign or by taking the `Viewport`, so rulers label document y correctly.
- The composition root stops negating y.
- `the_interface_page_edge_lands_on_the_rendered_page` still passes.
