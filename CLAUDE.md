# Xarast — project memory

A ground-up Rust reimplementation of **Xara Xtreme / Xara LX**: a vector
illustration and photo editor built for speed. Linux/Wayland first (shipped as
an AppImage), then Windows and macOS.

## Language rule (mandatory)

**Everything written in this repository is in English** — documentation, code,
comments, identifiers, commit messages, issue text, UI strings and doc files'
names. No exceptions. Conversation with the maintainer may be in Spanish; the
repository is not.

## Permanent facts (do not re-research)

- **Why the original cannot be ported.** All rasterisation lives in the
  **closed, binary-only** `libs/*/libCDraw.a` (its own licence in
  `libs/LIBS-LICENSE`; never opened). Only the headers `GDraw/gdraw.h`,
  `gdraw2.h` and `gconsts.h` exist. Without CDraw the program draws nothing, so
  **the engine has to be reimplemented**.
- The original targets **wxWidgets 2.6/2.8 + GTK2**, has no Wayland support, and
  ships 2006-era binaries for x86/x86_64/ppc/darwin-ppc only.
- The original is forked at **`/home/user/xara-xtreme`** (branch
  `claude/xara-xtreme-rust-port-ktggmu`). It is **read-only**: a normative
  reference for behaviour, never a codebase to translate.
- Size of the original: `Kernel/` 559 `.cpp` + 627 `.h`; `wxOil/` 182 `.cpp`.
- **`.xar` format**: defined in `Kernel/cxf*.{cpp,h}`. `Kernel/cxftags.h`
  defines **211** `TAG_*` names; counting `cxfdefs.h` and `basedoc.h` too there
  are **433** distinct names, of which **300** have a documented wire meaning
  and **157** actually occur in the corpus. The **59-file corpus** lives in
  `xara-xtreme/{testfiles,Designs,Templates,TextDesigns}/` — used locally via
  `XARAST_XAR_CORPUS`, **never copied into this repository**.
- **Native `.xarast` format**: a ZIP container wrapping **SVG** plus
  deduplicated binary resources. Requirement: it must open with graceful
  degradation in a browser or Inkscape, and with full fidelity in Xarast.
- **Licence: `MIT OR Apache-2.0`** (decided 2026-09-20, see
  `docs/11-licensing-and-clean-room.md`). The original is **GPL-2.0-only** (not
  "or later"), which makes GPL-3 *incompatible* with it; MIT can be folded into
  a GPL-2.0-only project while staying permissive, and Apache-2.0 adds a patent
  grant.
- **CLEAN ROOM — hard rule.** A permissive licence only holds if Xarast is
  **not a derivative work**. Read the original to *understand and document*;
  implement from `docs/research/*`, **never** by copying or translating code.
  Facts may be taken (tags, binary layouts, semantics, `file:line` references);
  function bodies, class definitions, comments and copyrighted assets may not.
  The "Xara*" marks belong to Xara Group Ltd: never imply affiliation.
- **Dependencies:** GPL/AGPL are banned; MPL-2.0 and LGPL need a written
  justification in `deny.toml`. `cargo deny check licenses` is mandatory in CI.

## Conventions

- **Branch:** `claude/xara-xtreme-rust-port-ktggmu` in both repositories. Never
  push anywhere else.
- **Units:** millipoints (`i32`) at the I/O boundary, as in Xara; the engine
  works in `f32`/`f64` per layer (see `docs/research/03-render-engine.md`).
- `unsafe` only at justified, documented FFI boundaries.
- The `.xar` parser must never panic or overflow on corrupt input; it is
  developed with `cargo-fuzz` from day one.
- Commits: small, imperative mood, prefixed by area (`xar:`, `core:`,
  `render:`, `ui:`, `pkg:`, `docs:`).

## Layout

- `docs/00-vision-and-scope.md` — vision, delivery scope, design principles.
- `docs/research/01..06` — normative specifications (xar format, document
  model, render engine, feature inventory, technology stack, xarast format).
- `docs/10-architecture.md` — crate architecture and data flow.
- `docs/11-licensing-and-clean-room.md` — licence and clean-room policy.
- `docs/phases/` — phase-by-phase execution plan.
- `docs/memory/` — durable per-subsystem notes; **update them as you close work**.

## Working rules for agents

1. Read `docs/memory/INDEX.md` and the note for the subsystem you touch before
   starting.
2. When you finish, **write or update** that note with: decisions taken, dead
   ends to avoid, invariants discovered.
3. Do not redo research already captured in `docs/research/`.
4. Keep the clean-room rule above at all times.
