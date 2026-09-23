---
id: XARA-US-0011
type: story
title: As a user, the canvas renders on the GPU backend
status: in_review
priority: high
parent: XARA-EP-0017
author: mcp
labels: [render, gpu, hardware]
created: 2026-09-23T09:40:31Z
updated: 2026-09-23T12:29:08Z
started: 2026-09-23T12:03:52Z
---

## Description
Phase 4 closed CPU-only. Remaining: WGSL compositing pass — paint evaluation, family dispatch, LUT sampling, ping-pong destination reads (R5.3, R5.4); wgpu adapter selection and the four-level capability ladder (S4/U2.5); frame pacing beyond `Wait`/`WaitUntil`.

## Acceptance Criteria
- GPU/CPU parity test passes on real hardware.
- Capability ladder degrades gracefully to CPU.
