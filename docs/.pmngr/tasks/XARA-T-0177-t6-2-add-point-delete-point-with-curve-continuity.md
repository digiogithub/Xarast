---
id: XARA-T-0177
type: task
title: T6.2 Add point / delete point with curve continuity
status: done
parent: XARA-US-0034
author: mcp
labels: [phase-7, tools, geometry]
created: 2026-09-23T19:00:17Z
updated: 2026-09-23T19:00:17Z
---

## Description
Click on a segment adds a point (de Casteljau split, curve unchanged); Delete/Backspace deletes selected points, rebuilding one cubic that keeps the outer tangents and recovers the split parameter (add∘delete = identity, proptest). Deleting every node deletes the object.

## Notes
Commits 48b92b6, f794094, ad19cff. Tests: xarast-geom/tests/path_edit.rs, xarast-app/tests/node_edit.rs.
