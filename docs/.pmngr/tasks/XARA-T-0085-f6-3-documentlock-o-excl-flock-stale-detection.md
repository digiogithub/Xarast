---
id: XARA-T-0085
type: task
title: F6.3 — DocumentLock (O_EXCL + flock, stale detection)
status: done
parent: XARA-US-0026
author: mcp
labels: [phase-6, xarast-format, durability]
created: 2026-09-23T14:23:38Z
updated: 2026-09-23T14:23:38Z
---

## Description
`.<name>.lock` with the §10.4 key/value text; `acquire[_with]`, `steal_if_stale`, `force`; `LockError::Held { holder, stale }` and `LockError::Unavailable` (read-only directory: open unlocked). Stale = same host + boot id + dead pid + free flock. Released on drop, only if the file is still ours (inode/device).

## Acceptance Criteria
- lock unit tests: exclusivity, dead local holder reclaimed, foreign host never stale, force, loser does not delete winner's lock, read-only directory.

## Notes
Done in commit 6e85a50. Uses `std::fs::File::try_lock` (no fs4).
