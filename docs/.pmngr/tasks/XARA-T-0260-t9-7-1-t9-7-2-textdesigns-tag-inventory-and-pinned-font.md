---
id: XARA-T-0260
type: task
title: "T9.7.1 + T9.7.2: TextDesigns tag inventory and pinned-font golden renders"
status: done
priority: high
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, acceptance]
created: 2026-09-24T01:24:31Z
updated: 2026-09-24T02:28:34Z
started: 2026-09-24T01:24:31Z
closed: 2026-09-24T02:28:34Z
---

## Description

Phase 9 acceptance criteria 2 and 3 for the 14 `TextDesigns/` files:

- `xar-dump --tags` over all 14 reports no unhandled text tag in 2100-2117, 2200-2204, 2900-2920, 4200-4207; the per-file inventory is recorded in `docs/memory/text.md`.
- `xarast-cli render` (CPU) renders all 14 with the pinned fonts, zero failures, nothing pending, and matches a committed golden **digest** exactly (gate A). The corpus is not in the repository, so neither are its renders: only derived digests.
- No missing-glyph (`.notdef`) glyph for any visible character.

## Acceptance Criteria

- `crates/xarast-cli/tests/text_designs.rs` asserts the three points above when `XARAST_XAR_CORPUS` is set; `XARAST_UPDATE_GOLDEN=1` rewrites the digests.
- `docs/memory/text.md` records the inventory and the golden policy for corpus renders.
