---
id: XARA-T-0062
type: task
title: Drop the ignored unmaintained advisories (ttf-parser via egui/ab_glyph, rustybuzz via resvg in xtask)
status: backlog
priority: medium
parent: XARA-US-0066
author: mcp
labels: [deps, licensing, ci]
created: 2026-09-23T14:13:09Z
updated: 2026-09-23T14:13:09Z
---

CI `cargo deny advisories` failed on 2026-09-23 with RUSTSEC-2026-0192 (`ttf-parser` unmaintained) and RUSTSEC-2026-0206 (`rustybuzz` unmaintained). Both are transitive: ttf-parser through egui 0.33 → epaint → ab_glyph (shipped) and resvg/fontdb (xtask); rustybuzz only through resvg/usvg in xtask. Ignored in `deny.toml` with reasons. Remove the ignores when egui moves off ab_glyph (check each egui upgrade) and when xtask's golden tooling moves to a maintained stack (e.g. resvg release on skrifa/harfrust).
