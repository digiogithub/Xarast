---
id: XARA-T-0077
type: task
title: F2.6/F2.7 — meta.xml model and its SVG metadata mirror
status: todo
parent: XARA-US-0022
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:22:56Z
updated: 2026-09-23T14:22:56Z
---

## Description
`DocumentMeta` (doc-id, revision, Dublin Core, units, pages, palette, guides/grid) with foreign-section preservation (research/06 §7, §8.2), and the `<metadata><rdf:RDF>` mirror in document.svg with meta.xml authoritative. Today the container carries meta.xml as opaque bytes (`PackageWriter::set_meta`, `XarastReader::meta_bytes`).

## Acceptance Criteria
- meta.xml round trip incl. unknown sections; `xmllint --relaxng meta.rnc`.

## Notes
Left over from round 1; fits with W3.
