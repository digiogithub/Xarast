---
id: XARA-T-0211
type: task
title: T8.2.7 — ApplyGroupTransparency / RemoveGroupTransparency
status: backlog
parent: XARA-US-0038
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:53:36Z
updated: 2026-09-23T19:53:36Z
---

## Description
Not in the model round. A transparency attribute on a group composited as a unit (render side is phase 4's layer push/pop). `SetFillGeometry { node: group, value: FillValue::Transparency(..) }` already writes the group's own attribute; what is missing is the command that marks it as group-level (and its removal), and the walker contract for compositing it as one layer.
