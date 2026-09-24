---
id: XARA-T-0291
type: task
title: "T10.8.2: retain embedded ICC profiles as document resources, and keep them through a PNG conversion at placement"
status: backlog
priority: medium
parent: XARA-US-0055
author: mcp
labels: [phase-10, color]
created: 2026-09-24T11:52:16Z
updated: 2026-09-24T11:52:16Z
---

## Description
The colour-space slot exists in `xarast-image` (sniffing, the assumed flag, the single `to_working_space` choke point) and the bitmap gallery reports each bitmap's colour space from a header probe. Still open:
- the document's `BitmapInfo` has no colour-space field;
- embedded profiles are not stored as `.xarast` resources under `resources/profiles/` (T10.8.2); PNG/JPEG/GIF keep theirs only inside their verbatim bytes;
- a WebP, TIFF, BMP or PNM placed through `place.rs` is converted to PNG by `xarast-io`, which writes an `sRGB` chunk and no `iCCP`, so its profile is lost.

## Acceptance Criteria
- Placing an image with an ICC profile keeps the profile (as a resource, or as `iCCP` in the converted PNG) and the gallery still says "ICC profile".
- `.xarast` writes and reads `resources/profiles/`, deduplicated by hash, round-trip tested.
- Nothing converts: the choke point stays the only place.

## Notes
If `qcms` (MPL-2.0) is ever pulled in, `deny.toml` needs a written justification.
