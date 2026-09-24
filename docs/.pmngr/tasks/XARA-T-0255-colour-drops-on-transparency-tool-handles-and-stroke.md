---
id: XARA-T-0255
type: task
title: Colour drops on transparency-tool handles and stroke gradients; line-colour undo label
status: backlog
parent: XARA-US-0042
author: mcp
labels: [phase-8, app]
created: 2026-09-23T23:50:12Z
updated: 2026-09-23T23:50:12Z
---

## Description
Gaps left by XARA-US-0042's drop resolution (`xarast-app/src/colour_bar.rs`):
- only the **fill tool's colour** handles are drop targets; a colour dropped on the transparency tool's handles (the original maps it to a level) or on a stroke's own gradient handles is not resolved;
- a wholly invisible leaf (no painted fill, no stroke) inside a group is not in the pick index, so it cannot take a drop;
- a line-colour click or drop is labelled "Set Fill" in the undo list, because `SetFillGeometry` names itself by payload, not by slot.
- deleting a named colour in use from the gallery detaches it silently; the original asks first.

## Acceptance Criteria
- Each point is either implemented with a test or recorded as a deliberate difference in `docs/memory/colour.md`.
