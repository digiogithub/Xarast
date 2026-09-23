---
id: XARA-T-0214
type: task
title: Keypad-only shortcuts lose to the same key bound without a location
status: done
parent: XARA-US-0084
author: mcp
labels: [ui, bug]
created: 2026-09-23T19:55:07Z
updated: 2026-09-23T19:55:07Z
---

## Description
Found by the new `every_chord_of_the_command_table_reaches_its_command` test after the phase-7 snapping merge: keypad `1` ran Zoom 100 % instead of Show guides, because `ShortcutMap::resolve` took the last matching binding and the unrestricted `1` was bound after the keypad one. A location-restricted match now wins regardless of bind order.

## Notes
Commit 7009fd7 (`crates/xarast-shell/src/input/keyboard.rs`).
