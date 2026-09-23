---
id: XARA-EP-0004
type: epic
title: Phase 3 — .xar importer
status: done
milestone: XARA-M-0002
author: mcp
labels: [phase-3]
created: 2026-09-23T09:39:43Z
updated: 2026-09-23T09:39:43Z
---

## Description
`xarast-xar` + `xar-dump`: physical reader, record tree, colours, paths, attributes, shapes, structure, bitmaps (encoded), text structure; the 59-file corpus imports into the model.
Spec: `docs/phases/phase-03-xar-importer.md`. Memory: `docs/memory/xar-import.md`.

## Notes
Closed (commit 7a23c80). Six fuzz targets compile but have never been run (needs nightly) — tracked in carry-over.
