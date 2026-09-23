---
id: XARA-T-0206
type: task
title: T8.2.1 — SetFillGeometry with inverse, for both payload types
status: done
parent: XARA-US-0038
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:53:36Z
updated: 2026-09-23T19:53:36Z
---

## Result
- Commit `48807d8` (`crates/xarast-doc/src/fill_edit.rs`).
- `SetFillGeometry { node, slot: PaintSlot, value: FillValue::{Colour, Transparency} }` writes the node's own attribute of the slot (fill, stroke colour, transparency fill, stroke transparency), replacing an attribute child or adding one first; editing an inherited fill localises it.
- Test: all 8 shapes × both slots × own/inherited attribute, both payloads, digest-exact undo and redo; locked objects refuse with `NotPermitted` and leave digest and history untouched.
