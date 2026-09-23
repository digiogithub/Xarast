---
id: XARA-T-0023
type: task
title: Nightly CI job running every fuzz target
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, ci]
created: 2026-09-23T10:26:21Z
updated: 2026-09-23T10:26:21Z
---

## Description
`.github/workflows/fuzz.yml`: matrix over all 11 targets, nightly toolchain + cargo-fuzz 0.13.2 (taiki-e/install-action), 300 s per target at 02:30 UTC (duration selectable on `workflow_dispatch`), `-timeout=10 -rss_limit_mb=2048 -malloc_limit_mb=1024`. Any crash/panic/OOM/timeout fails that target's job and uploads the reproducer; the grown corpus is carried in the Actions cache, never committed. Target dictionary picked up from `fuzz/dicts/<target>.dict`.

## Notes
The Fuzz step was dry-run locally (10 s) for `fuzz_svg_path_parse` (dictionary path) and `fuzz_xar_path` (seed path). Not yet exercised on GitHub runners.
