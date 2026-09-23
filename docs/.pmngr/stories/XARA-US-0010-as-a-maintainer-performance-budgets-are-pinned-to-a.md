---
id: XARA-US-0010
type: story
title: As a maintainer, performance budgets are pinned to a reference machine
status: backlog
priority: critical
parent: XARA-EP-0017
author: mcp
labels: [perf, hardware]
created: 2026-09-23T09:40:31Z
updated: 2026-09-23T09:40:31Z
---

## Description
Pick a reference machine (integrated GPU + Wayland compositor) and re-measure everything that the container could not.

## Acceptance Criteria
- Budget table in `docs/memory/perf.md` pinned to the machine.
- Rasteriser spike gates G1 and G2 (phase 4 W0) re-run and settled.
- Pan/zoom ≤ 16 ms @100k objects, open 5 MB `.xar` ≤ 500 ms, cold start ≤ 400 ms measured.
- `preorder` and `Tree::get` re-measured (currently within 40 % of budget).
