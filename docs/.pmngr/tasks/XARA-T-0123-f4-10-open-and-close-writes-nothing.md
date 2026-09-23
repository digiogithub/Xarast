---
id: XARA-T-0123
type: task
title: F4.10 Open and close writes nothing
status: done
priority: medium
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
`open` reads only: no lock file, no temporary, the file's bytes and mtime untouched.

## Notes
Done in 71fe240; `tests/svg_read.rs::opening_writes_nothing`.
