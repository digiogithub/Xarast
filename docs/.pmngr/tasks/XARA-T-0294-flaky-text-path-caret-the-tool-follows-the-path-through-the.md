---
id: XARA-T-0294
type: task
title: "Flaky: text_path_caret the_tool_follows_the_path_through_the_session failed once in a workspace run"
status: todo
priority: high
author: mcp
labels: [text, test]
created: 2026-09-24T12:06:55Z
updated: 2026-09-24T13:17:53Z
---

## Description
During XARA-T-0281's gates, `XARAST_XAR_CORPUS=… cargo test --workspace` failed once in `crates/xarast-app/tests/text_path_caret.rs:353` (`the_tool_follows_the_path_through_the_session`): caret at (1212814.0, 1052089.0) vs expected (1211518.2, 1050715.9), about 1.9 pt off. It passed alone right after, and in a second full workspace run (1891 passed, 0 failed). The machine was under load 60–80 from other agents. T-0281 touched no text code.

## Acceptance Criteria
- Find what makes the caret position depend on the run (font service state shared across tests in the binary, test order, fontconfig timing) and make the test deterministic.

## Notes
Seen on branch worktree-agent-aceb5c4b6760b91d4 at 454b3dd.
