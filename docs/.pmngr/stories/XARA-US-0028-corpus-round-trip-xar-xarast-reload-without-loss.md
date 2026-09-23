---
id: XARA-US-0028
type: story
title: "Corpus round-trip: .xar → .xarast → reload without loss"
status: backlog
priority: high
parent: XARA-EP-0007
author: mcp
labels: [phase-6, acceptance]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T09:41:28Z
---

## Acceptance Criteria
- All 59 corpus files round-trip with identical model and render.
- Save of a 20 MB `.xarast` ≤ 1 s.
- `docs/memory/xarast-format.md` created and filled.
