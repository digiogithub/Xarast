---
id: XARA-T-0313
type: task
title: Document the crash-report privacy rule and safe mode in the user manual
status: backlog
parent: XARA-US-0064
author: mcp
labels: [phase-12, stability, docs]
created: 2026-09-24T17:46:34Z
updated: 2026-09-24T17:46:34Z
---

## Description
Phase 12 §F says the rule "no document content and no file paths outside the document's basename" is written in the code comment **and in the user manual**. The code comment exists (`crates/xarast-app/src/crash.rs`). The manual (G1) does not exist yet.

## Acceptance Criteria
- The troubleshooting chapter says where reports are kept (`$XDG_STATE_HOME/xarast/crashes/`) and what they hold and never hold. It says they are never sent, and explains `--safe-mode`, the automatic offer and the forced crash-loop mode.

## Notes
Depends on G1 (mdBook manual). Filed from XARA-US-0064.
