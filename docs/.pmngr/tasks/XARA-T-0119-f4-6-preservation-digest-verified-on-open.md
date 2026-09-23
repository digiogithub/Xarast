---
id: XARA-T-0119
type: task
title: F4.6 Preservation digest verified on open
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
`SvgRead::preservation` / `OpenedDocument::preservation`: the declared `xarast:foreign-count`/`foreign-digest` against what the model now carries, recomputed by running `write_svg` (the writer's emission order, no second implementation). A lower count is the §8.4 warning ("N items … have been lost"); same or higher but different is an Info.

## Notes
Done in 967da9a; `tests/svg_read.rs::the_digest_notices_foreign_data_another_program_dropped`.
