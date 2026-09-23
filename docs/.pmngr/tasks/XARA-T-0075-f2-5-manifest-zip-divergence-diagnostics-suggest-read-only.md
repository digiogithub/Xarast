---
id: XARA-T-0075
type: task
title: F2.5 — Manifest/ZIP divergence → diagnostics, suggest read-only
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
`XarastReader::diagnostics()` / `suggests_read_only()`: unlisted entry, missing entry, duplicate row, size mismatch, missing mandatory digest, digest mismatch, bad root row, out-of-order entry, resource name/digest mismatch, unsupported method.

## Acceptance Criteria
- `manifest_divergence_is_diagnosed_not_fatal`.

## Notes
Done in commit 0529f6d. The UI that shows them is the app's (see F6.4 task for the lock UX counterpart).
