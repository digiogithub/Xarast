---
id: XARA-US-0081
type: story
title: As a user, the viewer is robust and renders corpus files faithfully
status: done
priority: high
parent: XARA-EP-0006
author: mcp
labels: [phase-5, render, shell]
created: 2026-09-23T10:36:03Z
updated: 2026-09-23T12:32:44Z
closed: 2026-09-23T12:32:44Z
---

## Description
Follow-ups found while wiring the first viewer (XARA-US-0001) and the CLI corpus run. Holds the tasks moved out of US-0001 when it closed.

## Acceptance Criteria
- wgpu validation errors never crash the app.
- Quick shapes, gradient-heavy files and SimpleSphere render correctly and within budget.
- Headless API gaps closed; rulers read correctly.
