---
id: XARA-T-0008
type: task
title: Measure pan/zoom frame time at 100k objects on the reference machine
status: done
priority: high
parent: XARA-US-0010
author: mcp
labels: [perf, hardware]
created: 2026-09-23T10:00:29Z
updated: 2026-09-23T13:24:30Z
closed: 2026-09-23T13:24:30Z
---

## Description
Budget: pan/zoom ≤ 16 ms per frame with 100 000 objects (roadmap performance table). Needs the wired viewer (shell + ui + app composed), so it was deferred from the first reference-machine round of XARA-US-0010.

Reference machine: Intel Core Ultra 9 285 (8P + 16E), 93 GiB, NVIDIA RTX 4000 SFF Ada (580.173.02) + Intel Arrow Lake iGPU (Mesa 26.1.6), COSMIC Wayland, kernel 7.1.5. See `docs/memory/perf.md`.

## Acceptance Criteria
- Frame time p50/p99 over a scripted pan and a scripted zoom on a 100k-object document (synthetic `bulk` and `ProbeX16.xar`), measured present-to-present on the live Wayland session.
- Measured on the CPU backend, and on the GPU backend once XARA-US-0011 lands; both adapters (iGPU and discrete) recorded.
- Numbers added to the budget table in `docs/memory/perf.md` with load conditions noted.

## Notes
Context from round 1: the raw `vello` GPU raster of `bulk` 1080p is ~5.7–8.2 ms on the RTX 4000 but 33–80 ms on the Arrow Lake iGPU; the CPU production path (`CpuBackend`, 100k) is in `perf.md`. `DisplayList::build` at 100k is ~21 ms (XARA-US-0016), which alone breaks 16 ms if it is rebuilt per pan frame.
