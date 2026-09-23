---
id: XARA-T-0174
type: task
title: "Text rendering leftovers: faux bold, font underline metrics, text in bounds caches"
status: backlog
parent: XARA-US-0045
author: mcp
labels: [phase-9, text]
created: 2026-09-23T18:48:35Z
updated: 2026-09-23T18:48:35Z
---

## Description

Gaps left by the text-render round (see `docs/memory/text.md`, Open TODOs):

- Faux **bold** is not synthesised (`FontMatch::synthesis.embolden` ignored; faux italic skew is applied).
- Underline uses fixed proportions (0.1 × size below the baseline, 0.05 × size thick), not the face's `post` underline metrics (`FaceMetrics::underline`).
- Text stories have no cached bounds: `drawing_rect` lays them out only while the layer's bounds cache is cold, `Viewport::fit_bounds_to` (scroll bounds) and hit testing ignore text. A derived text-bounds cache (outside the arena) should feed all three.
- The ruler's decimal-point character is not in the model (needed by T9.3.8).
- A per-document embedded-font overlay (today an embedded face is visible to every document in the shared `FontDb`).

## Acceptance Criteria

- Each item fixed or split into its own task with a test.
