---
id: XARA-T-0024
type: task
title: Run the six .xar fuzz targets on nightly for the first time
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, xar]
created: 2026-09-23T10:26:28Z
updated: 2026-09-23T10:26:28Z
---

## Description
10 minutes each, one process per target, seeded with the committed synthetic corpus plus (locally, read-only) the 59 real files. Phase-3 criteria 17 and 18.

## Results
| Target | Execs | exec/s | Findings |
|---|---|---|---|
| fuzz_xar_records | 299 k | 497 | none |
| fuzz_xar_tree | 237 k (clean rerun) | 394 | DOWN-twice drops children (XARA-T-0011) |
| fuzz_xar_decode | 523 k | 870 | none |
| fuzz_xar_import | 389 k (clean rerun) | 646 | three accounting bugs (XARA-T-0006, -0007, -0011) |
| fuzz_xar_path | 70.2 M | 116 750 | none |
| fuzz_xar_colour | 5.8 M | 9 706 | none |

No panic, overflow or OOM anywhere; every finding was a structure/accounting bug visible only to the stronger assertions.
