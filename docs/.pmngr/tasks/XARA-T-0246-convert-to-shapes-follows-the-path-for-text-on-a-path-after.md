---
id: XARA-T-0246
type: task
title: Convert to shapes follows the path for text on a path (after W9.5)
status: done
parent: XARA-US-0049
author: mcp
labels: [phase-9, text]
created: 2026-09-23T22:35:23Z
updated: 2026-09-24T00:15:02Z
started: 2026-09-23T23:30:56Z
closed: 2026-09-24T00:15:02Z
---

## Description
Convert to shapes bakes what the walker draws. Text on a path is still drawn straight until W9.5 (`text_on_path_pending`), so converting `Designs/TextCurve.xar`'s story yields straight outlines. Once W9.5 lands, `story_outlines` gets the on-path geometry for free (it reuses `build_story`); add a test on TextCurve.xar then.

## Acceptance Criteria
- Converting TextCurve.xar's story gives outlines matching its on-path render (zero differing pixels).
