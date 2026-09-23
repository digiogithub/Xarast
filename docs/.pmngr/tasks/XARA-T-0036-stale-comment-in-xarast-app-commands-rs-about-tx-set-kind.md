---
id: XARA-T-0036
type: task
title: Stale comment in xarast-app commands.rs about Tx::set_kind repairing the active layer per call
status: done
priority: low
parent: XARA-US-0081
author: mcp
labels: [app, docs]
created: 2026-09-23T11:23:17Z
updated: 2026-09-23T11:26:36Z
closed: 2026-09-23T11:26:36Z
---

## Description
The comment at `crates/xarast-app/src/commands.rs:107` says `Tx::set_kind` runs `keep_one_active_layer` after every call. That has not been true since commit 2005f67, which moved the repair to commit time. Since XARA-T-0030 (commit 432c607), the commit also checks only the spreads the transaction touched. The comment should describe that behaviour, and any workaround it justifies should be re-checked.

## Acceptance Criteria
- The comment matches `docs/memory/document-model.md`: repairs run once per commit, and only on the spreads the transaction touched.

## Notes
This is in the app-core crate. The document-model owner did not edit it.
