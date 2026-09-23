---
id: XARA-T-0073
type: task
title: F2.3 — Roles with unknown-role tolerance and preservation
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
`Role::Other(String)` keeps an unknown role verbatim; `effective_role()` reads it as `unknown`.

## Acceptance Criteria
- `unknown_roles_attributes_and_children_are_preserved`, `unknown_entries_are_carried_byte_for_byte`.

## Notes
Done in commit 0529f6d.
