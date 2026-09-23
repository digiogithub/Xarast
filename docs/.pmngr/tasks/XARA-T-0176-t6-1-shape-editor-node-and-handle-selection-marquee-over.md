---
id: XARA-T-0176
type: task
title: "T6.1 Shape editor: node and handle selection, marquee over nodes, move"
status: done
parent: XARA-US-0034
author: mcp
labels: [phase-7, tools]
created: 2026-09-23T18:59:52Z
updated: 2026-09-23T18:59:52Z
---

## Description
Shape editor (F4): click/Adjust/Adjust+Constrain node selection, marquee, select all points, Esc; node drag (45° constrain), handle drag with smooth sync, segment reshape; one labelled SetPath step per drag; point selection outside PathData.

## Notes
Commits 48b92b6 (geom EditPath), ad19cff (tool). Tests: crates/xarast-app/tests/node_edit.rs.
