---
id: XARA-T-0245
type: task
title: T9.6.5 — Outline fallback for export when a font cannot be embedded (profile C, PDF/SVG)
status: in_review
parent: XARA-US-0049
author: mcp
labels: [phase-9, text]
created: 2026-09-23T22:35:23Z
updated: 2026-09-24T08:51:14Z
started: 2026-09-24T08:50:57Z
---

## Description
Use the same outline path (`xarast_app::text::story_outlines` / `convert_story_to_shapes` geometry) in `.xarast` conformance profile C and in PDF/SVG export when a face's fsType forbids embedding. Not done in the W9.6 round (XARA-US-0049): that round delivered the edit command and source-text preservation only.

## Acceptance Criteria
- A document using an embedding-denied face exports to SVG/PDF with the text as outlines carrying `xarast:was-text`, rendering identically to the text.

## Notes
Touches xarast-io/xarast-format export; coordinate with the export owners.
