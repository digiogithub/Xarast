---
id: XARA-US-0068
type: story
title: A/B — Live-object infrastructure and shared offscreen + blur pipeline
status: backlog
parent: XARA-EP-0014
author: mcp
labels: [phase-13, live-effects, render]
created: 2026-09-23T09:43:25Z
updated: 2026-09-23T09:43:25Z
---

## Description
Foundation for every live effect. Today 1 594 corpus records round-trip as `NodeKind::Opaque` and `live_pending` > 0.

## Tasks (full tables: phase-13 §A, §B)
- A1–A4 `NodeKind::Live`, Source/Generated invariants, `regenerate`, `RegenQueue`.
- B1–B4 `LayerTarget`, `render_subtree_to_layer`, alpha extraction, Disc + Gaussian blur.
- Decisions: blend steps materialised vs generated; `MouldGeometry` trait vs enum; `ProceduralSource` cache hash.
