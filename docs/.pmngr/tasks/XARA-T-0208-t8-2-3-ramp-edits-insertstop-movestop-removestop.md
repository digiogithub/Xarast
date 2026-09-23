---
id: XARA-T-0208
type: task
title: "T8.2.3 — Ramp edits: InsertStop, MoveStop, RemoveStop, SetStopValue"
status: done
parent: XARA-US-0038
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:53:36Z
updated: 2026-09-23T19:53:36Z
---

## Result
- Commit `48807d8`. Pure helpers `ramp_insert`, `ramp_move` (returns the stop's new index), `rebuild_ramp`, `set_stop`, `stop_value`.
- `SetStopValue` targets From/To/Mid(i)/Corner(i); a transparency value keeps its stop's mode; mismatched payloads are refused.
- Acceptance 10: setting one intermediate stop changes that field and nothing else (field-by-field diff test).
- Property test: random sequences of the four edits keep stops sorted and in `0..=1`, never touch from/to except through From/To targets, and undoing everything returns the starting digest.
