# Clean-room hygiene report — `docs/research/`

> **Date:** 2026-09-20
> **Scope:** the six research documents `01`–`06` of `docs/research/`.
> **Reason:** Xarast will be released under a **permissive licence
> (MIT OR Apache-2.0)**. The original, Xara Xtreme / Xara LX, is
> **GPL-2.0-only**. The permissive licence is only legitimate if we maintain
> **clean-room** discipline: we may document *facts* (file formats, field names
> and types, semantics, tag numbers, algorithms described in prose) but we may
> **not reproduce *expression*** from the original's source code.

## Classification criteria applied

| Class | What it is | Action |
|---|---|---|
| **(A)** | A literal or near-literal copy of the original: function bodies, class definitions with their C++ syntax, member declaration lists, constant tables verbatim. | **Rewritten** as English prose, a markdown table or neutral pseudocode, **keeping** the `file:line` reference. |
| **(B)** | An interface fact or a datum of the format: tag numbers, binary layouts, field lists with type and meaning, file-format constants. | **Kept**, but presented as a markdown table or pseudocode, never as copied C++. Comments rewritten in our own words. |
| **(C)** | Rust / WGSL / pseudocode proposed by us, `.xarast` XML, TOML/YAML from our build, examples from other projects, dumps from our own tools. | **Kept as is**. |

## Summary per document

| Document | Blocks reviewed | Rewritten (A) | Kept as fact (B) | Our own (C) |
|---|---:|---:|---:|---:|
| `01-xar-format.md` | 36 | 6 | 20 | 10 |
| `02-document-model.md` | 122 (116 + 6 indented) | 70 | 13 | 39 |
| `03-render-engine.md` | 31 | 10 | 13 | 8 |
| `04-feature-inventory.md` | 0 | 0 | 0 | 0 |
| `05-technology-stack.md` | 15 | 0 | 1 | 14 |
| `06-xarast-format.md` | 52 | 0 | 2 | 50 |
| **Total** | **256** | **86** | **49** | **121** |

In addition, the **clean-room notice** was added just below the H1 title of all
six documents, adapted in a single sentence to the content of each one.

Subsequent verification: **no block tagged `cpp`, `c` or `c++` remains** in
`docs/research/`, nor any untagged block that gets past the original's C++
syntax detector (`BOOL`, `INT32`, `UINT32`, `TCHAR`, `DocRect`, `DocCoord`,
`virtual`, `public:`, `CC_DECLARE_*`, `#define`, `#include`, `::`, `new X(`…).
Every comment remaining inside the blocks that were kept is in Spanish and is
our own wording.

---

## Detail of the rewritten blocks

### `01-xar-format.md` — 6 blocks

| Block (§) | Original content | Rationale for the rewrite |
|---|---|---|
| §1.2 signature | two `#define`s of `CXF_IDWORD1/2` (`cxfdefs.h:109-110`) | The values are a fact of the format, but the `#define` form is expression: moved to a constant/value/origin table. |
| §1.6 file types | three `#define EXPORT_FILETYPE_*` (`camfiltr.h:143-145`) | Likewise: the literals `CXN`/`CXW`/`CXM` are an interoperability fact; the table keeps them without the C syntax. |
| §2.3 unknown records | payload-discard loop (`cxfile.cpp:1997-2065`) | A function body: replaced by two lines of neutral pseudocode that say exactly the same thing. |
| §3.2 compression version | two statements computing and writing the zlib version (`cxfile.cpp:705-742`) | A function body: replaced by the formula in prose (`major*100 + minor`, emitted as `u32`). |
| §3.3 zlib parameters | literal calls to `deflateInit2`/`inflateInit2` (`zstream.cpp:490-545`, `:760-770`) | Although they are calls into a third-party API, they were copied verbatim from the original: converted into a table of parameters with each value and its effect, which is also more useful for reimplementing. |
| §7.4 path verbs | five `const BYTE PT_*` (`gconsts.h:108-112`) | Format constants (a fact) re-expressed as a name/value/role table, with the note that bits 3-7 are unused. |

### `02-document-model.md` — 70 blocks

The 65 blocks tagged `cpp` were, almost without exception, **lists of member and
virtual-method declarations extracted from the original's headers** (plus four
function bodies and three indented blocks inside lists). All of them have been
converted into **markdown tables** of the form
"field / type / meaning / `file:line`", or into step-by-step prose. No name, no
type and no cross-reference has been lost; in several cases information was
gained (ranges, units, invariant notes).

| §  | Block | Rationale |
|---|---|---|
| 1.2 | ~60 virtual fast-type predicates (`node.h:455-520`) | Class declarations → predicate/question/reference table. |
| 1.3 | `NodeFlags` + `Node` link fields (`node.h:758-784`) | Struct definition with bitfields → two tables (flags and fields). |
| 2.x | Parametric state of `NodeRegularShape` (`nodershp.h:299-322`) | Member list → table, marking what is cache. |
| 3.1 | Body of `Document::InitTree` (`document.cpp:415`) | **Function body**: rewritten as a numbered 5-step procedure with the `:line` references intact. |
| 3.2 | Fields of `NodeDocument` (`nodedoc.h:164-168`) | Declarations → table. |
| 3.2 | Fields of `Spread` (`spread.h:329-383`) | Declarations → table. |
| 3.2 | State of `Layer` (`layer.h:341-380`) | Declarations → two tables (layer state and GIF frame state). |
| 3.2 | `switch` in `Layer::EnableLayerCacheing` (`layer.cpp:481`) | **Function body**: rewritten as a table of the three caching levels plus one sentence about cutting the traversal short. |
| 4.1 | The `Value` / `GetAttributeValue()` pattern (`fillattr2.h:132-138`) | Declarations → one sentence describing the pattern. |
| 4.1 | Interface of `AttributeValue` (`attrval.h:134-173`) | Virtual declarations → method/role table. |
| 4.3 | State fields of `RenderRegion` (`rndrgn.h:904-935`) | Declarations → table. |
| 4.3 | Fields of `AttributeEntry` (`attrmgr.h:127`) | Declarations → table. |
| 4.3 | Five `RR_*` macros (`rndrgn.h:971-1008`) | **Macros from the original**: replaced by a description of the mechanism (index the array, cast, return the field) + a macro/slot/value table. |
| 4.3 | Interface of `RenderStack` (`rndstack.h:128-132`) | Declarations → operation/effect table. |
| 4.4 | Static registry of default attributes (`attrmgr.h:251-276`) | Declarations → table. |
| 4.4 | Loop in `Document::InitDefaultAttributeNodes` (`document.cpp:520`) | **Function body**: rewritten as prose. |
| 4.5 | `AttributeGroup` (`attrmgr.h:161`) + `NUM_ATTR_GROUPS` | Class definition → table + one sentence. |
| 5.x | Point/colour/transparency accessors of graduated fills (`fillval.h`) | Declarations → table of accessor families. |
| 5.x | `enum RepeatType` | Enumeration → value/name/meaning table (the values are a fact of the format). |
| 5.x | `enum TranspType` (`fillval.h:144`) | Likewise, with a note on which values are not legal in the document and a pointer to §2.7 of `03`. |
| 5.x | `RampItem` / `ColRampItem` / `TranspRampItem` / `FillRamp` (`fillramp.h:134-243`) | Class definitions → class/reference/contents table. |
| 5.x | Profile accessors of `CProfileBiasGain` (`fillval.h:328-332`) | Declarations → one sentence. |
