---
id: XARA-T-0300
type: task
title: "geometry: gate f64 cross-architecture determinism (x86_64 vs aarch64 CI runner)"
status: backlog
priority: low
parent: XARA-US-0061
author: mcp
labels: [geometry, phase-12, ci]
created: 2026-09-24T13:11:09Z
updated: 2026-09-24T13:11:09Z
---

## Description
geometry.md: "Cross-architecture f64 determinism is not gated … promote to a gate in phase 12". Not done in XARA-US-0061: it could not be verified locally (no aarch64 host). The AppImage workflow already uses `ubuntu-24.04-arm`.

## Acceptance Criteria
- A test that digests geometry outputs (flattening, booleans, bounds, offsets over fixed inputs and the synthetic document) against a pinned value, run on x86_64 and on `ubuntu-24.04-arm`.
- Any libm-dependent difference (sin/cos/atan2 in arcs) found and either removed or documented.
- Render/export byte identity across architectures assessed separately (vello_cpu SIMD paths differ).
