---
id: XARA-T-0027
type: task
title: Make wgpu validation errors non-fatal in the shell (on_uncaptured_error)
status: backlog
priority: high
parent: XARA-US-0001
author: mcp
labels: [shell]
created: 2026-09-23T10:31:45Z
updated: 2026-09-23T10:31:45Z
---

## Description
By default wgpu 30 panics on any uncaptured validation error ("Handling wgpu errors as fatal by default"). During XARA-US-0001, a surface reconfigure while a texture was still held killed the window on `Designs/Groucho2.xar`; that bug is fixed in 330c7d1. The next driver quirk or pass bug will still take the whole document down with it.

## Acceptance Criteria
- `Gpu::new` installs `Device::on_uncaptured_error`. The handler logs the error, counts it and asks the loop to recover: reconfigure the surface, then fall back to skipping frames.
- A validation error never aborts the process, and ui.md invariant 6 ("a diagnosis, never a panic") covers the GPU path.
