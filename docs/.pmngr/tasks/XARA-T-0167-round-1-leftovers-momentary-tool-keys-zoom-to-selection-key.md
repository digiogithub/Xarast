---
id: XARA-T-0167
type: task
title: "Round-1 leftovers: momentary tool keys, zoom-to-selection key, layer-lock checks in commands"
status: done
parent: XARA-US-0032
author: mcp
labels: [phase-7, tools, ui]
created: 2026-09-23T18:10:55Z
updated: 2026-09-23T18:10:55Z
---

## Notes
89e808b (locked layers refuse every EditCommand), 48c2e73 (View › Zoom to selection = `3`; Ctrl+Shift+Z stays Redo), 4aaf2b4 (Space/Alt+S selector, Alt+Z zoom, Alt+X push, held; release of the same key or focus loss restores). Tests: `a_locked_layer_refuses_every_edit`, `input/momentary.rs`, viewer `held_switch_keys_change_the_tool_until_released`.
