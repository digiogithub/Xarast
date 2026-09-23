---
id: XARA-T-0071
type: task
title: F2.1 — Manifest model, quick-xml parser and writer
status: done
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
`manifest::Manifest` with namespace `https://xarast.org/ns/manifest/1.0` resolved by hand (in-scope bindings are needed for foreign fragments). DTD, undefined entities, invalid XML chars and invalid names refused; depth capped; unbound prefixes refused. Canonical writer (fixed attribute order and prefixes). Foreign attributes, unknown `mf:` attributes and verbatim namespace-complete foreign fragments are carried.

## Acceptance Criteria
- Parses the §12.4 example; write→parse is a fixed point after one round (also fuzz-asserted).

## Notes
Done in commit 0529f6d. A fuzz finding (quick-xml accepts `a="b"` glued into an element name) is fixed there: names are validated as QNames.
