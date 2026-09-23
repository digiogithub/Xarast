---
id: XARA-T-0086
type: task
title: F6.4 — Lock UX and SIGINT/SIGTERM lock cleanup (app)
status: done
parent: XARA-US-0026
author: mcp
labels: [phase-6, xarast-app, durability]
created: 2026-09-23T14:23:38Z
updated: 2026-09-23T19:53:37Z
closed: 2026-09-23T19:53:37Z
---

## Description
In xarast-app/ui: on `LockError::Held` offer "Open read-only" / "Open a copy" / "Force (risky)" (`DocumentLock::force`); on `Unavailable` open unlocked. Install SIGINT/SIGTERM handlers that drop held `DocumentLock`s (research/06 §10.4 forbids relying on atexit alone). Also surface `XarastReader::diagnostics()` / `suggests_read_only()`.

## Notes
Left over from round 1; the format crate has no unsafe and no signal dependency.
