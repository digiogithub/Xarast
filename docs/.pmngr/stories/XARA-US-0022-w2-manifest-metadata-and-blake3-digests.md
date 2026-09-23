---
id: XARA-US-0022
type: story
title: W2 — Manifest, metadata and BLAKE3 digests
status: done
priority: high
parent: XARA-EP-0007
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T15:01:24Z
started: 2026-09-23T14:22:07Z
closed: 2026-09-23T15:01:24Z
---

## Tasks (full table: phase-06 §W2)
- F2.1 `Manifest` model with `quick-xml`, namespace `https://xarast.org/ns/manifest/1.0`.
- F2.2 Root `/` entry; one entry per ZIP entry.
- F2.3 Roles with unknown-role tolerance.
- F2.4 Streaming BLAKE3-256 digests (mandatory for document/meta/resource).
