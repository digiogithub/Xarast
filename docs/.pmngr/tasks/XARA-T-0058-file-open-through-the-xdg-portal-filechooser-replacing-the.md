---
id: XARA-T-0058
type: task
title: File › Open through the XDG portal FileChooser, replacing the current document
status: done
priority: critical
parent: XARA-US-0082
author: mcp
labels: [phase-5, shell]
created: 2026-09-23T13:50:18Z
updated: 2026-09-23T14:14:13Z
started: 2026-09-23T13:50:18Z
closed: 2026-09-23T14:14:13Z
---

## Description
The UI raises an open request as an `Intent`; the shell performs it with `PortalService` (filters "Xara documents (*.xar)" and "All files"). The chosen file replaces the current document (single-document model). Errors (portal failure, import failure) show in the status bar and the problem list, never a panic; a failed open keeps the current document.

## Acceptance Criteria
- Tested with an offline `PortalService` and synthetic portal answers; no test posts to a live portal.
- Manual checklist for the live dialog left in the story comment.
