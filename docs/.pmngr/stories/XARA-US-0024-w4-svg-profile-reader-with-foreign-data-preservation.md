---
id: XARA-US-0024
type: story
title: W4 — SVG profile reader with foreign-data preservation
status: done
priority: high
parent: XARA-EP-0007
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T17:09:24Z
started: 2026-09-23T15:49:20Z
closed: 2026-09-23T17:09:24Z
---

## Description
As a user, editing a `.xarast` in another tool and reopening it in Xarast never loses art or foreign data.

## Tasks (full table: phase-06 §W4)
- F4.1 Preserving `quick-xml` reader, DTD rejected.
- F4.2 Base + parametric pair; regenerate `xarast:generated` subtrees.
- F4.3 Orphan baked subtrees kept as editable geometry, with a warning.
- F4.4 Capture foreign attributes, elements, comments and PIs verbatim.
