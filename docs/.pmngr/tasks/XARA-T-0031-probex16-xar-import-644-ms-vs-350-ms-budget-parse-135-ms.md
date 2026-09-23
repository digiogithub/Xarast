---
id: XARA-T-0031
type: task
title: ProbeX16.xar import 644 ms vs 350 ms budget (parse 135 ms; model build dominates)
status: done
parent: XARA-US-0016
author: mcp
labels: [xar, perf]
created: 2026-09-23T10:37:22Z
updated: 2026-09-23T11:22:57Z
started: 2026-09-23T11:03:05Z
closed: 2026-09-23T11:22:57Z
---

From the reference-machine run; bench in `crates/xarast-xar/benches/import.rs`. Profile where the remaining ~500 ms goes after parsing.
