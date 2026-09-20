# Xarast licensing and clean-room policy

> Decided 2026-09-20. This document is **normative**: every contribution, human
> or agent, must comply with it.

---

## 1. The decision

**Xarast ships under `MIT OR Apache-2.0`** — dual licensed at the recipient's
choice, the Rust ecosystem convention.

The brief was: *a licence compatible with the original, the one that grants the
most freedom*. `MIT OR Apache-2.0` satisfies both constraints at once, and is
the combination that maximises them.

---

## 2. Why this one

### 2.1 What "the original" actually is

Xara LX is **GPL-2.0-only** — not "or later". Its `LICENSE` says so literally:

> *"under the terms of the GNU General Public License version 2 as published by
> the Free Software Foundation"*

It adds a **linking exception** for wxWidgets, wxXtra and the binary CDraw
library, and reserves the "Xara", "Xara LX" and "Xara Xtreme" marks.

That it is `only` and not `or later` is the crux of everything below.

### 2.2 Licence compatibility runs one way

Compatibility is not symmetric. What each option would actually allow:

| Xarast licence | Can our code be folded into a GPL-2.0-only project? | Can we use the Rust ecosystem (Apache-2.0)? |
|---|---|---|
| **MIT** | Yes | Yes |
| **Apache-2.0** | **No** — its patent clause is incompatible with GPL-2.0-only | Yes |
| **MIT OR Apache-2.0** | Yes, via the MIT arm | Yes |
| GPL-2.0-only | Yes | **No** — locks us out of Apache-2.0 crates |
| GPL-3.0-or-later | **No** — GPL-2.0-only and GPL-3.0 cannot be combined | Yes |

Two consequences rule out the alternatives:

- **GPL-3.0-or-later** (the first proposal made in this project) is the *least*
  compatible option with the original, not the safest: GPL-2.0-only and GPL-3.0
  **cannot be combined at all**. It would have been a mistake.
- **GPL-2.0-only** would maximise compatibility and minimise freedom, and would
  additionally shut us out of much of the Rust ecosystem, which is
  overwhelmingly Apache-2.0.

`MIT OR Apache-2.0` delivers what was asked: recipients can do essentially
anything with Xarast — including folding it into a GPL-2.0-only project through
the MIT arm, or into proprietary software — while we stay free to use any
permissive crate. The Apache-2.0 arm adds an explicit **patent grant** that MIT
lacks.

### 2.3 The condition that makes this choice valid

A permissive licence is **only legitimate if Xarast is not a derivative work**
of Xara LX. Copying or translating GPL-2.0-only code would bind us to
GPL-2.0-only, and publishing under MIT would then be an infringement.

Licence and clean room are therefore inseparable: **the clean-room policy is
not a nicety, it is the precondition that holds the licence up.**

---

## 3. Clean-room policy

### 3.1 What MAY be taken from the original

These are **facts**, not creative expression, and they are required for
interoperability:

- Numbers, names and semantics of `.xar` **record tags**.
- **Binary layouts**: field order, sizes, endianness, units.
- Field names and their meaning, where they describe an **interface** or a data
  format.
- **Observable behaviour**: what a tool does, what an effect produces, the order
  in which attributes apply.
- **Algorithms described in prose** or restated as our own pseudocode.
- **Cross-references** such as `Kernel/document.cpp:415`. Citing where logic
  lives is legitimate and invaluable for verification.

### 3.2 What MUST NOT be taken

- Function bodies, copied or translated line by line.
- Class definitions with their syntax and ordering.
- Comments from the original.
- Constant tables copied verbatim where they are not format data.
- Copyrighted assets: icons, bitmaps, clipart, UI strings, help files. **The
  `.xar` files in `testfiles/` and `Designs/` are used locally as a validation
  corpus only; they are never redistributed with Xarast.**
- The "Xara" marks and derivatives — not in the product name, the UI, materials,
  or in any way that suggests affiliation.

### 3.3 The operating rule

> Read the original to **understand** and **document**. Implement from the
> documentation (`docs/research/*`), not from the code.

In practice: if you are writing Rust with a `.cpp` open beside you, mirroring
its structure, you are doing it wrong. If you are writing Rust from
`docs/research/01-xar-format.md`, you are doing it right.

The research documents have been through a **hygiene audit**
(`docs/research/00-hygiene-report.md`) that rewrote any fragment sitting too
close to the original and kept format facts as tables and pseudocode.

### 3.4 Naming and trademarks

"Xarast" is a new name. Even so, to avoid any confusion with Xara Group Ltd's
marks:

- Nothing states or implies continuity, affiliation or endorsement.
- `LICENSE` carries the trademark disclaimer.
- The string "Xara" appears only in descriptive, truthful contexts ("imports
  Xara Xtreme files"), which is legitimate nominative use.

---

## 4. Acceptable third-party licences

With `MIT OR Apache-2.0` as the project licence:

| Dependency licence | Acceptable? | Note |
|---|---|---|
| MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unlicense, CC0 | Yes | No friction |
| MPL-2.0 | With care | Per-file copyleft. Acceptable when used unmodified. Affects e.g. `resvg`/`usvg` |
| LGPL (dynamic linking) | Avoid | Complicates the AppImage and static linking |
| **GPL / AGPL** | **No** | Would infect the whole binary and break the permissive licence |
| No declared licence | No | |

**Automated enforcement:** `cargo deny check licenses` in CI, with the allow
list in `deny.toml`. No dependency lands without passing it.

Known watch items: the render engine design considers `resvg`/`usvg` (MPL-2.0),
and some compression backends are dual BSD/GPL-2 (`zstd`). Both must be recorded
in `deny.toml` with their justification.

---

## 5. Repository files

| File | Contents |
|---|---|
| `LICENSE` | Dual-licence statement plus the Xara disclaimer |
| `LICENSE-MIT` | MIT text |
| `LICENSE-APACHE` | Apache-2.0 text |
| `deny.toml` | Machine-checkable licence policy for CI |
