---
id: XARA-T-0261
type: task
title: "T9.7.5: multi-script test documents with no .notdef (Arabic, Devanagari, Thai, CJK)"
status: backlog
priority: medium
parent: XARA-US-0050
author: mcp
labels: [phase-9, text, acceptance]
created: 2026-09-24T01:27:28Z
updated: 2026-09-24T01:27:28Z
---

## Description

Phase 9 T9.7.5 / acceptance criterion 15, filed from XARA-US-0050. The pinned test fonts (`crates/xarast-text/tests/fonts/`) cover Latin, Hebrew, Arabic and CJK subsets but **no Devanagari and no Thai** face, so a synthetic Latin + Arabic + Devanagari + Thai + CJK document cannot render without `.notdef` in tests today.

## Acceptance Criteria

- Add OFL subsets of Noto Sans Devanagari and Noto Sans Thai to the pinned set (provenance and SHA-256 in `PROVENANCE.md`, `make_subsets.sh` updated), without moving the existing shaping goldens.
- A test builds a document mixing the five scripts in Xarast (no corpus bytes), lays it out with the pinned fonts and asserts no `PlacedGlyph` has id 0 (as `xarast-cli/tests/text_designs.rs` does for the corpus).
- The same document round trips through `.xarast` pixel-identical.
