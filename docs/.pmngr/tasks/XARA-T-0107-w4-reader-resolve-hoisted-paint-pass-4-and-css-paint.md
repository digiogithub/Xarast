---
id: XARA-T-0107
type: task
title: "W4 reader: resolve hoisted paint (pass 4) and CSS paint classes (pass 5)"
status: done
priority: high
parent: XARA-US-0024
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T16:23:14Z
updated: 2026-09-23T17:05:18Z
closed: 2026-09-23T17:05:18Z
---

## Description
Since XARA-T-0101 (commits d8897af, e7e0eb4 on `claude/xara-xtreme-rust-port-ktggmu`) the SVG writer no longer puts the full resolved paint on every ink element. The reader must resolve it back. Normative contract: `docs/memory/xarast-format.md` § "Passes 4–5: hoisted paint and CSS classes — the reader contract". Summary:

**Slot properties** (the only ones affected): `fill`, `fill-opacity`, `fill-rule`, `xarast:fill-ref`, `stroke`, `stroke-opacity`, `stroke-width`, `stroke-linecap`, `stroke-linejoin`, `stroke-miterlimit`, `stroke-dasharray`, `stroke-dashoffset`, `xarast:stroke-ref`. `vector-effect`, `opacity`, `mask`, `style` (mix-blend-mode), `xarast:blend` stay on the element as before.

1. **Inheritance (pass 4).** A property's value is: the element's own attribute; else its class rule (2); else the nearest ancestor `<g>`'s attribute or class; else the SVG initial value. `xarast:fill-ref` / `xarast:stroke-ref` are **inherited the same way** (initial = no palette reference). Values may sit on any `<g>`: group, layer, the first spread, ClipView, live-effect `<g>`.
2. **Classes (pass 5).** `<style type="text/css">` is the first child of `<defs>`, one rule per line: `.NAME{prop:value;prop:value}` (class selectors only, slot properties minus the twins, values spelled exactly as attributes). Twins are **not** in the CSS: `<xarast:paint-class xarast:class="NAME" xarast:fill-ref="#c-N" xarast:stroke-ref="#c-M"/>` right after the `<style>`. `class="NAME"` applies both. NAME is `c1, c2…` unless foreign baggage already uses `c<digits>` — then `xc`, `xrc` or `xarast-c`: take names from the `<style>`, do not assume `cN`. Classes appear on ink elements and on `<g>`s.
3. An element never has both the attribute and a class for the same property.
4. Localise (XARA-T-0105): after resolution an ink element's paint is its complete resolved set; a `<g>`'s own hoisted paint is **not** a model attribute of the group — drop it once pushed down.
5. Inside `<clipPath>` (ClipView clip shapes) paint is inline, no classes.

Output written before round 3 (no hoisting, no classes) is a special case of this contract; both must read identically.

## Acceptance Criteria
- A document saved with `SvgOptions { hoist: true, classes: true }` (the default) and with both off reads back to the same model (normalised as in XARA-T-0105).
- Unit tests for: a value inherited through two `<g>` levels; a twin inherited from a `<g>`; a class with twins from `<xarast:paint-class>`; a non-`c` prefix; an element with its own attribute under a `<g>` with a different value.
- Corpus round trip (59 files) unchanged by the two passes.

## Notes
Writer side: `crates/xarast-format/src/svg/style.rs`. The `<style>` is always `.NAME{…}` lines only; a tolerant reader should still ignore anything else in it (third-party edits).
