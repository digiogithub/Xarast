---
id: XARA-T-0053
type: task
title: "Bounds cache: an attribute edit leaves its siblings' cached boxes stale"
status: backlog
priority: low
parent: XARA-US-0029
author: mcp
labels: [document-model]
created: 2026-09-23T12:28:41Z
updated: 2026-09-23T12:32:44Z
---

## Description
Found while fixing the stroke-aware framing in `xarast-app::viewport::drawing_rect` (XARA-US-0081 sweep). This is a code reading, not a reproduced bug.

`Tree::invalidate_bounds` (`crates/xarast-doc/src/tree.rs`) marks the edited node invalid, then climbs its ancestors. An object's box depends on its stroke extent, which comes from the attribute nodes in force. A line-width or join attribute that sits on a **group** applies to every following sibling inside that group. Editing that attribute invalidates the attribute node and the group's ancestors. It does not invalidate the sibling paths, whose cached boxes still hold the old extent, and the group's box is then rebuilt from those stale child boxes.

A second point: the climb stops at a node that has **no** cache entry (`None => break`). If the edited node was never cached, its valid ancestors keep their old boxes.

Today nothing fills the cache after import (`Document::update_bounds` is only called by tests), so the viewer does not see this yet. Once something warms the cache (Phase 7 editing, selection bounds), it will show as wrong selection or fit boxes after changing a line width on a group.

## Acceptance Criteria
- Changing an attribute invalidates the boxes of every node it applies to (its following siblings and their subtrees, plus the parent's own ink), or the cache stops depending on inherited attributes.
- The climb does not stop at a node that is merely uncached.
- A test: `update_bounds`, change a group-level line width through a command, and check that the cached box of a path inside the group equals a fresh computation.

## Notes
Owner: whoever owns `xarast-doc`. `xarast-app` never writes the cache (walker invariant 3). `drawing_rect` reads the cache only when it is valid, and otherwise walks with an attribute stack.
