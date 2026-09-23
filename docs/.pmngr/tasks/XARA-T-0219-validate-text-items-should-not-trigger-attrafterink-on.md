---
id: XARA-T-0219
type: task
title: "validate: text items should not trigger AttrAfterInk on multi-run lines"
status: backlog
priority: low
parent: XARA-US-0045
author: mcp
labels: [phase-9, text, doc]
created: 2026-09-23T20:03:12Z
updated: 2026-09-23T20:03:12Z
---

## Description

In the W9 model, attributes inside a `TextLine` are scoped to the following siblings, so attributes between `TextItem`s are how a story expresses runs (the `.xar` importer and the `.xarast` reader since XARA-T-0172 both build them). `validate` counts `TextItem` as ink (`NodeKind::is_ink`), so every multi-run line raises `Invariant::AttrAfterInk` and the builder adds the Info diagnostic "an attribute follows an ink node in a child list" on every `.xarast` open with such text.

## Acceptance Criteria

- Attributes between text items in a line do not produce `AttrAfterInk`.
- The import snapshot's validateWarnings column is updated.
