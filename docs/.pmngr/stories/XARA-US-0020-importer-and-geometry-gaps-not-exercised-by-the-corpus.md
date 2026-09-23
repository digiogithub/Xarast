---
id: XARA-US-0020
type: story
title: Importer and geometry gaps not exercised by the corpus
status: backlog
priority: low
parent: XARA-EP-0018
author: mcp
labels: [xar, geometry]
created: 2026-09-23T09:40:52Z
updated: 2026-09-23T09:40:52Z
---

## Description
- Finding 10 model gaps: one-extra-axis `Linear` fill, "extra" tiling, twenty predefined dash patterns.
- Legacy regular shapes (1000–1217, 1900) — currently dropped silently.
- Absolute path tags 100–103 as top-level nodes; `TAG_PATHREF_*` (118, 4013) dedup.
- `TAG_DEFINESOUND_WAV` (70), contoned bitmap nodes (199), XPE bitmap props (4117/4118).
- Overprint 3500–3505 pairing (unanswerable from corpus).
- Geometry: mixed-run refitting after boolean cuts (needs corner detection); `cavalier_contours` offset spike; `HitIndex` grid vs BVH at 100k segments; `insta` SVG snapshots; f64 cross-arch determinism gate (phase 12).
