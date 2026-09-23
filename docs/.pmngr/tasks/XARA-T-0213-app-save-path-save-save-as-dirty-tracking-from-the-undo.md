---
id: XARA-T-0213
type: task
title: "App save path: Save/Save As, dirty tracking from the undo clean point, confirm dialogs"
status: done
parent: XARA-US-0084
author: mcp
labels: [phase-6, app-core, ui]
created: 2026-09-23T19:55:07Z
updated: 2026-09-23T19:55:07Z
---

## Description
`History::state_serial` (xarast-doc) as the clean point; `Session::save_job` snapshots, `SaveJob`/`SaveWorker` write off the UI thread; `AppState` Save/SaveAs/SaveTo/SaveDialogClosed/AnswerPrompt and `ShowSaveDialog`; Close/Quit/Open/window close ask Save/Discard/Cancel; `.xar` saves as `.xarast`; title `• name — Xarast`; status bar notices; egui modal prompts.

## Notes
Commits e49a5f6, b099fe4, 2323b50, b9e7d48. Tests: `crates/xarast-app/tests/save.rs`, `crates/xarast-ui/tests/saving.rs`, viewer tests. Ctrl+Shift+S is Save As; Convert to editable shapes moved to Ctrl+Shift+C.
