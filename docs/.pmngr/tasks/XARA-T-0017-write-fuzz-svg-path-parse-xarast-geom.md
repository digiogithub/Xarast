---
id: XARA-T-0017
type: task
title: Write fuzz_svg_path_parse (xarast-geom)
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, geometry]
created: 2026-09-23T10:23:36Z
updated: 2026-09-23T10:23:36Z
---

## Description
SVG path data as arbitrary UTF-8 text through `Path::from_svg_path_data`, with a token dictionary (`fuzz/dicts/svg_path_parse.dict`). Accepted paths must validate, and `to_svg_path_data` -> parse must round-trip exactly.

## Notes
Found the arc-radius OOM (see the separate finding task).
