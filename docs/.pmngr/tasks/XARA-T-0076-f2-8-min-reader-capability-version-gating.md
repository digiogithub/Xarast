---
id: XARA-T-0076
type: task
title: F2.8 — min-reader / capability version gating
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
Other major version → `ReadError::UnsupportedMajor`; `min-reader` newer than 1.0 → `Diagnostic::NewerFormat` (read-only suggested); every declared capability → `MissingCapability` (non-optional ones suggest read-only). The writer declares none yet.

## Acceptance Criteria
- `newer_min_reader_and_capabilities_are_diagnosed`.

## Notes
Done in commit 0529f6d.
