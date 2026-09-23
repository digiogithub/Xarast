---
id: XARA-T-0065
type: task
title: "F1.3 — Reader: 64-byte signature sniff; open without parsing document.svg"
status: done
parent: XARA-US-0021
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:34Z
updated: 2026-09-23T14:22:34Z
---

## Description
`sniff`/`sniff_bytes` check PK\3\4, flags, STORED, sizes 26, name length 8, extra 0, `mimetype`, MIME at 38. `XarastReader::open` reads the end record, central directory and manifest only.

## Acceptance Criteria
- `open_does_not_parse_the_document` (a non-XML document.svg opens cleanly).

## Notes
Done in commit 0529f6d.
