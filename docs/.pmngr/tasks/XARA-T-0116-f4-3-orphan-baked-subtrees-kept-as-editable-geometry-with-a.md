---
id: XARA-T-0116
type: task
title: F4.3 Orphan baked subtrees kept as editable geometry with a warning
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
A `xarast:generated` subtree whose `generated-by` does not resolve to its enclosing controller becomes a plain group of ordinary objects, with a warning that the live effect is lost. Art is never deleted.

## Notes
Done in 967da9a; `tests/svg_read.rs::orphan_baked_geometry_is_kept_as_objects`.
