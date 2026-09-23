---
id: XARA-T-0195
type: task
title: T8.1.1 — ColourValue conversions RGB↔HSV↔grey↔CMYK↔sRGB with round-trip property tests
status: done
parent: XARA-US-0037
author: mcp
labels: [phase-8, color]
created: 2026-09-23T19:51:48Z
updated: 2026-09-23T19:51:48Z
---

## Description
Conversions matching the original's shipped (non-CMS) formulas, documented in `docs/research/02-document-model.md` §5.10.1.

## Result
- Commits: `4580d9b` (research), `d0b8d80` (code), `cab0944` (bench) on `worktree-agent-abb102d2f54edacb4`.
- RGB→CMYK now generates black past 50 % (exactly invertible); grey weights 0.305/0.586/0.109; HSV grey threshold 1e-6; `pack_component`/`to_rgba8_packed` reproduce the FIXED24 byte packing; `ColourContext` added.
- Round-trip error (units of 1/255): RGB→HSV 0.00009, RGB→CMYK 0, HSV→CMYK 0, CMYK→HSV 0.00009, grey→any 0.00003 (bounds 1 and 2).
- Corpus oracle: 5 760/5 760 palette entries resolve to their cached RGB exactly (round-to-nearest would miss 545).
- `ColourContext::convert` 6.5–9 ns (budget 20 ns).
