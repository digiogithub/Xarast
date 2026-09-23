---
id: XARA-T-0122
type: task
title: F4.9 Duplicate and foreign ids reassigned with a warning
status: done
priority: medium
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, xarast-doc]
created: 2026-09-23T17:04:57Z
updated: 2026-09-23T17:04:57Z
---

## Description
Each element with a canonical Xarast id claims its tag once (`DocumentBuilder::tag`, 16f8305); a duplicate, non-canonical or foreign id gets a fresh tag and an Info diagnostic.

## Notes
Done in 967da9a; `tests/svg_read.rs::duplicate_ids_are_reassigned_with_a_warning`. Keeping foreign ids other data refers to: XARA-T-0112.
