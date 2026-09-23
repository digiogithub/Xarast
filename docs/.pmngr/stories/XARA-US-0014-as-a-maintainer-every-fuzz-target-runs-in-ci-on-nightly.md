---
id: XARA-US-0014
type: story
title: As a maintainer, every fuzz target runs in CI on nightly
status: done
priority: high
parent: XARA-EP-0018
author: mcp
labels: [fuzz, xar, geometry, render]
estimate: 5
created: 2026-09-23T09:40:52Z
updated: 2026-09-23T10:36:14Z
started: 2026-09-23T09:50:43Z
closed: 2026-09-23T10:36:14Z
---

## Description
CLAUDE.md requires the `.xar` parser to be fuzzed from day one; six targets compile with committed seeds but have never run (no nightly in the container). Several targets are still unwritten.

## Acceptance Criteria
- Nightly CI job runs all targets for a bounded time (`fuzz/README.md` has the commands). Phase-3 criteria 17 and 18 met.
- New targets written: `fuzz_path_boolean`, `fuzz_svg_path_parse` (geometry), `fuzz_display_list`, `fuzz_ramp` (render), `DocumentBuilder` (doc model).
