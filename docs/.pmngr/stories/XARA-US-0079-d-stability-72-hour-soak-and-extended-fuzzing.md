---
id: XARA-US-0079
type: story
title: "D — Stability: 72-hour soak and extended fuzzing"
status: backlog
parent: XARA-EP-0016
author: mcp
labels: [phase-15, stability, fuzz]
created: 2026-09-23T09:43:25Z
updated: 2026-09-23T09:43:25Z
---

## Tasks (full table: phase-15 §D)
- D1 Seeded synthetic-user soak harness.
- D2 72 h soak per platform: zero crashes, RSS growth < 2 %.
- D3 Leak gates for every subsystem since phase 12.
- D4 Weekly 8 h fuzzing per importer target; `fuzz_pdf_parse`, `fuzz_emf_parse`.
