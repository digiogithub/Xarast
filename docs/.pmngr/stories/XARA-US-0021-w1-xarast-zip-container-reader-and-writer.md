---
id: XARA-US-0021
type: story
title: W1 — .xarast ZIP container reader and writer
status: backlog
priority: high
parent: XARA-EP-0007
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T09:41:28Z
---

## Description
As a user, my documents are stored in a robust, reproducible ZIP container.

## Tasks (see `docs/phases/phase-06-xarast-format.md` §W1 for the full table)
- F1.1 `zip` 8.x (deflate+zstd), deflate pinned to `zlib-rs`.
- F1.2 Writer: normative entry order, `mimetype` first, STORED, no extra field.
- F1.3 Reader: 64-byte signature sniff; open without parsing `document.svg`.
- F1.4 Entry-name validation — reject, never sanitise.
