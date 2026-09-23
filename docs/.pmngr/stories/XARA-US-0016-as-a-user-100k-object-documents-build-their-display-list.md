---
id: XARA-US-0016
type: story
title: As a user, 100k-object documents build their display list within budget
status: done
priority: high
parent: XARA-EP-0018
author: mcp
labels: [render, perf]
estimate: 3
created: 2026-09-23T09:40:52Z
updated: 2026-09-23T12:01:26Z
started: 2026-09-23T11:08:01Z
closed: 2026-09-23T12:01:26Z
---

## Description
`DisplayList::build` costs 106 ns/command: ~10.6 ms for 100k nodes vs a 3 ms budget. Cause: size of `DrawCmd`. Box the stroke payload first. Also: `AttrResolver` drops its whole cache on any change (slow mid-drag).

## Acceptance Criteria
- 100k-node build ≤ 3 ms on the reference machine.
