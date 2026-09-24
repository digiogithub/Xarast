---
id: XARA-T-0288
type: task
title: "perf: font service start (~41 ms, mostly fontconfig) paid by the first document with text"
status: backlog
priority: low
author: mcp
labels: [perf, text]
created: 2026-09-24T11:06:56Z
updated: 2026-09-24T11:06:56Z
---

## Description
Found in XARA-T-0287: Spitfire's first open went from 4.5 ms to ~46 ms at 7f44eac (text stories in the walker). The cost is a once-per-process font service start (~41 ms, mostly fontconfig) paid by the first document containing text; re-opening in the same process is 3.8 ms.

## Acceptance Criteria
- Start the font service off the UI/render critical path (e.g. warm it on a background thread at app start), or explain why not.
- First-open time of a text document measured before/after in perf.md.
