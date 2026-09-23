---
id: XARA-T-0114
type: task
title: F4.1 Preserving quick-xml reader for document.svg (DTD rejected, limits)
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T17:04:30Z
updated: 2026-09-23T17:04:30Z
---

## Description
`svg/read/dom.rs`: quick-xml with `check_end_names`, no trimming, QName validation, prefixes resolved by hand, UTF-8 only, BOM handling, DOCTYPE and entities refused, depth/element/byte limits; per-element byte spans and own namespace declarations for verbatim fragments.

## Notes
Done in 967da9a (worktree branch of the W4 round). Tests: `dom.rs` unit tests, `tests/svg_read.rs::hostile_xml_is_refused`, `fuzz_xarast_svg_read` (ca6430f).
