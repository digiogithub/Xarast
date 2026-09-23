---
id: XARA-T-0014
type: task
title: Gradient-heavy corpus files take ~10 s each to render at 100 % on the CPU backend
status: backlog
priority: medium
parent: XARA-US-0001
author: mcp
labels: [render, perf]
created: 2026-09-23T10:04:32Z
updated: 2026-09-23T10:04:32Z
---

## Description
`xarast-cli render` of the whole corpus at 100 % (release, deterministic CPU config, Final quality) takes about 27–33 s of render time. **About 31 s of that comes from three files**:

| File | Size | Commands | Render |
|---|---|---|---|
| `testfiles/10000GradFilledShapes.xar` | 766x739 | 10 001 | ~10.2 s |
| `testfiles/20000GradFilledShapes.xar` | 766x880 | 20 000 | ~10.9 s |
| `testfiles/20000GradFilledShapes50PCtransparent.xar` | 766x880 | 20 000 | ~9.8 s |

The median for the other 56 files is under 1 ms, and the next-slowest is `SimpleSphere.xar` at 0.66 s.

- `--quality draft` only brings the first file down to 6.5 s.
- At 192x192 it takes 0.45 s, so the cost scales with pixels × gradient objects.

That suggests per-object full-surface work: a layer, a ramp evaluation over the whole bbox, or no tile binning for gradient paints. `docs/memory/perf.md` records 24.6 ms for 20 000 flat-filled objects at 1080p, so gradients are roughly 400x slower per frame. Measured on an Intel Core Ultra 9 285 (24 threads).

## Acceptance Criteria
- Profile one of these files and find where the time goes.
- Bring it down to within a small multiple of the flat-fill number, or record why it cannot be.
