---
id: XARA-T-0259
type: task
title: GUI save passes the text placer so saved .xarast shows placed text in browsers
status: done
priority: high
parent: XARA-US-0084
author: mcp
labels: [phase-6, xarast-format, text]
created: 2026-09-24T01:08:08Z
updated: 2026-09-24T01:20:40Z
started: 2026-09-24T01:09:42Z
closed: 2026-09-24T01:20:40Z
---

## Description
`crates/xarast-app/src/save.rs` saves `.xarast` without the SVG text placer that `xarast-cli convert` and SVG export use (`crates/xarast-app/src/svg_text.rs`). A file saved from the app therefore has no browser-visible placed text (exact runs, per-character rotate on a path) in its base SVG, although Xarast itself reopens it exactly.

## Acceptance Criteria
- App Save / Save As / autosave pass the same text placer as `xarast-cli convert`.
- Bytes of an app save equal a CLI convert of the same document (or the difference is explained).
- Snapshot serialisation stays off the UI thread budget (~54 ms on ProbeX16) or the cost is measured and recorded.

## Notes
Found during XARA-T-0252.
