---
id: XARA-T-0306
type: task
title: "Import: map TAG_CLIPVIEWCONTROLLER/TAG_CLIPVIEW (4084/4085) onto NodeKind::ClipView"
status: in_progress
priority: low
parent: XARA-US-0020
author: mcp
labels: [xar, import]
created: 2026-09-24T15:03:45Z
updated: 2026-09-24T16:08:20Z
started: 2026-09-24T16:08:20Z
---

## Description
Found in XARA-US-0017. The `.xar` importer does not map the ClipView controller and node (4084/4085, and the declared-but-unimplemented 4137) onto `NodeKind::ClipView`; the corpus has **0** such records (raw-record census, `docs/memory/xar-import.md` finding 16), so nothing exercises it.

The original's `NodeClipViewController` keeps the clipping object as its **topmost** (last) child (`docs/research/02-document-model.md` §6.11), while our model takes the **first** child as the clipping path, so the importer must reorder. The original has no "keep the outside" mode: an imported ClipView is always `ClipViewMode::Inside`.

The walker now draws ClipViews in both modes (US-0017, `app-core.md` decision 43), so this is purely an importer gap.

## Acceptance Criteria
- A synthetic `.xar` (`xarast_xar::synth::XarBuilder`) with a ClipView controller imports as a `ClipView` whose first child is the original's topmost object, and renders clipped (pixel probe).
- The fill census in `crates/xarast-cli/tests/fills.rs` still matches.
