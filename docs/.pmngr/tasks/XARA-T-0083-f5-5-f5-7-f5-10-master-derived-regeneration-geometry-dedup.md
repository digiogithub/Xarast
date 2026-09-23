---
id: XARA-T-0083
type: task
title: "F5.5/F5.7/F5.10 — master/derived regeneration, geometry dedup, data: URI policy"
status: todo
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:23:15Z
updated: 2026-09-23T14:23:15Z
---

## Description
All three need the SVG layer (W3): `--no-derived` regeneration path from `<xarast:photo-ops>` (the index already records `derived_from`/`derivation` and the manifest carries them), `<defs>`+`<use>` geometry dedup (≥ 3 repeats or ≥ 512 bytes of `d`), `data:` URIs only under 4 KiB.

## Notes
Left over from round 1.
