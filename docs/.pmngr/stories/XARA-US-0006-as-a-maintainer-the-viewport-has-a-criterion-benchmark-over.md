---
id: XARA-US-0006
type: story
title: As a maintainer, the viewport has a criterion benchmark over 100k objects
status: done
parent: XARA-EP-0006
author: mcp
labels: [phase-5, perf, app-core]
estimate: 2
created: 2026-09-23T09:40:08Z
updated: 2026-09-23T11:26:36Z
started: 2026-09-23T11:22:25Z
closed: 2026-09-23T11:26:36Z
---

## Description
Acceptance criterion 4: `cargo bench -p xarast-app -- viewport` does not exist yet.

## Acceptance Criteria
- Bench uses the 100 000-object synthetic document from `xarast_doc::synth` and a scripted pan/zoom.
- Result recorded in `docs/memory/perf.md`.
