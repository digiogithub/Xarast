---
id: XARA-T-0020
type: task
title: "Fix: SVG arc with a huge radius makes kurbo allocate 1.8 GB (fuzz_svg_path_parse)"
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, geometry]
created: 2026-09-23T10:24:02Z
updated: 2026-09-23T10:24:02Z
---

## Description
`Path::from_svg_path_data("M18-7A1  8170073e71 11 111\n 15")` (30 bytes) made kurbo approximate an arc of radius 8e77 with ~10^13 cubics: a 1.8 GB allocation. Arc radii are not coordinates, so the post-parse extent check never saw them.

## Fix
`check_svg_magnitudes` scans every number (SVG number grammar) before kurbo parses, refusing any beyond twice the document extent, which bounds the radii and so the approximation. Regression test `svg_reader_refuses_numbers_beyond_the_extent_before_parsing` in `crates/xarast-geom/tests/paths.rs`.
