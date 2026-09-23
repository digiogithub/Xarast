---
id: XARA-T-0113
type: task
title: "F4.7 deletion accounting: warn on save when objects carrying foreign data were deleted"
status: backlog
priority: low
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, xarast-app]
created: 2026-09-23T16:46:55Z
updated: 2026-09-23T16:46:55Z
---

## Description
`research/06 §8.5` rule 4: deleting an object takes its baggage with it, and the save warning says "N objects with data from future versions deleted". The marking half of F4.7 is done (`Tx::transform`/`set_attr`/`set_kind`/attribute `attach` mark `DIRTY`/`STALE`); the count is not: a save does not know what was deleted since the file was opened.

## Acceptance Criteria
- A save reports how many objects with baggage were deleted since open (e.g. `OpenedDocument` remembers the baggage-carrying tags; `save_opened` counts those no longer reachable) and the application shows it.
