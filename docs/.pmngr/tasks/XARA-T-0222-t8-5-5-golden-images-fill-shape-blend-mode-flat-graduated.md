---
id: XARA-T-0222
type: task
title: "T8.5.5 — golden images: fill shape × blend mode × flat/graduated"
status: backlog
parent: XARA-US-0040
author: mcp
labels: [phase-8, render]
created: 2026-09-23T21:18:03Z
updated: 2026-09-23T21:18:03Z
---

## Description
The phase-8 golden matrix: every fill shape × every exposed transparency blend mode × {flat, graduated}, rendered through the CPU backend and locked as golden images.

## Acceptance Criteria
- The matrix is generated and compared in CI with the existing golden tooling.
