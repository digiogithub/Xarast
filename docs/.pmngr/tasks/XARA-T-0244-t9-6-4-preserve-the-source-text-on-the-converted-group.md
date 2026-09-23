---
id: XARA-T-0244
type: task
title: T9.6.4 — Preserve the source text on the converted group (xarast:was-text)
status: in_review
parent: XARA-US-0049
author: mcp
labels: [phase-9, text]
created: 2026-09-23T22:35:23Z
updated: 2026-09-23T22:41:21Z
started: 2026-09-23T22:35:23Z
---

## Description
`GroupNode::source_text`; written as `xarast:was-text="true"` + `<xarast:text-source>` (research/06 §6.7), read back, in the digest, the dump and the normal form.

## Acceptance Criteria
- svg_read fixture round-trips a source text with markup and a paragraph break.
- Every converted TextDesigns group keeps its text after .xarast save/reopen, same pixels.

## Notes
Commits cb0b3d1, 6986c9d.
