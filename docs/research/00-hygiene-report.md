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
| 5.x | Interface of `CProfileBiasGain` (`biasgain.h:170-243`) | Declarations → operation/role table. |
| 5.8 | Fields of `FractalFillAttribute` (`fillval.h:913`) | Declarations → table with ranges and units. |
| 5.8 | Fields of `NoiseFillAttribute` | Likewise. |
| 5.8 | Fractal/noise generation and cache (`fillval.h:274-344`) | Declarations → operation/role table. |
| 5.x | Fill distortion and blending interface (`fillval.h:288-337`) | Declarations → table. |
| 5.x | Fields of `DocColour` (`doccolor.h`) | Declarations → table. |
| 6.x | `ObjChangeFlags` flags (`objchge.h`) | Bitfields → flag/what-it-announces table. |
| 6.x | `enum OpPermissionState` | Enumeration → a sentence with the three values and where they are encoded. |
| 6.x | Condition in `NodeCompound::OnChildChange` (`nodecomp.cpp:272-320`) | **Function body**: rewritten as the rule it implements, in one sentence. |
| 6.x | Loop in `Application::RegenerateNodesInList` (`app.cpp:1830-1880`) | **Function body**: neutral pseudocode + an explanation of why it invalidates the box twice. |
| 6.5 | "Tight groups" (`group.h:204-209`, indented block) | Declarations → prose, keeping the 72,000/dpi formula. |
| 6.x | Boxes of `NodeRenderableBounded` (`node.h:1425-1440`) | Declarations → table. |
| 6.x | Bitmap-caching interface (`node.h:1448-1450` and around it) | Declarations → table + a sentence on the three static switches. |
| 6.x | `CBitmapCacheKey`, `CCachedBitmap`, `CBitmapCache` (`bitmapcachekey.h:104`, `bitmapcache.h:114-161`) | Class definitions → three tables + a description of the maximum-size policy. |
| 6.6 | State of `NodeBlend` (`nodeblnd.h:129`) | Declarations → table. |
| 6.6 | `BlendPath` and `BlendRef` (`nodebldr.h:140`, `:287`) | Class definitions → table + prose. |
| 6.7 | Interface of `MouldGeometry` (`moldshap.h`) | Virtual declarations → operation/role table. |
| 6.8 | Contour state (`ncntrcnt.h:153` and derivatives) | Declarations → table, highlighting that the sign of the width encodes inside/outside. |
| 6.9 | Shadow state (`nodecont.h:215` and derivatives) | Declarations → table, grouping the "resolution it was generated at" fields. |
| 6.10 | Bevel state (`nbevcont.h:125` and derivatives) | Declarations → table. |
| 6.11 | ClipView state (`ndclpcnt.h:146`) | Declarations → table. |
| 6.12 | `NodeEffect` / `NodeBitmapEffect` (`nodepostpro.h:130`, `nodeliveeffect.h:163`) | Tree with member declarations → class/base/state table. |
| 6.13 | `CanBecomeA` / `DoBecomeA` (`node.h:656-657`) | Declarations → prose, explaining what the parameter carries. |
| 7.2 | State of `TextStory` (`nodetxts.h:456-460` ff.) | Declarations → table, with the units made explicit. |
| 7.3 | `TextStoryInfo` | Declarations → table. |
| 7.3 | `TextLineInfo` (`nodetxtl.h:253`) | Declarations → table, keeping the note that the sum of advances excludes the last tracking value. |
| 7.3 | Character positioning parameters | Declarations → table, marking which ones are input constants. |
| 7.3 | Cached attributes of `TextLine` | Declarations → table, keeping the note on why they are cached. |
| 7.4 | `VisibleTextNode` (`nodetext.h:166-201`) | Declarations → table + prose for the predicates. |
| 7.4 | Metrics of `AbstractTextChar` (`nodetext.h:271-278`) | Declarations → table. |
| 7.5 | Aborted primitives of `FormatRegion` (`nodetxtl.h:130`) | **Body with `ERROR3`**: rewritten as the sentence "the format region only measures, it never paints". |
| 7.5 | Accessors of `FormatRegion` (`nodetxtl.h:173` ff.) | Declarations with inline bodies → query/returns/origin table. |
| 8.1 | Fields of `KernelBitmap` (`bitmap.h:627-634`) | Declarations → table. |
| 8.2 | Fields of `BitmapInfo` (`bitmpinf.h`) | Declarations → table. |
| 8.3 | JPEG without recompression: `WritePalette` / `Convert24To8` (`bitmap.h:514-516`, indented block) | Declarations → prose. |
| 8.3 | `KernelBitmapRef` (`bitmap.h:664-673`) | Declarations → table + a description of reference counting by presence in the tree. |
| 8.5 | Fields of `NodeBitmap` (`nodebmp.h:180`) | Declarations → table. |
| 9.1 | `RangeControl` (`range.h:219`) + `Range` / `SelRange` | Bitfields and classes → table + sentence. |
| 9.1 | Cache of `SelRange` | Declarations → table, keeping the warning that the counter is invalid if the range is not cached. |
| 9.2 | `Operation` (`ops.h:323-404`) | Class definition → table grouped by families of operation. |
| 9.2 | Flags of `UndoableOperation` | Bitfields → table. |
| 9.3 | `Action` (`ops.h:559-608`) | Class definition → member/role/reference table. |
| 9.3 | `ActionList` (`ops.h:196-208`) | Declarations → prose. |
| 9.4 | Fields of `OperationHistory` (`ophist.h:219-232`) | Declarations → table, keeping the fact that the budget is **in bytes**. |
| 9.4 | Interface of `OperationHistory` | Declarations → grouped table. |
| 9.5 | Enumerations and copy operations (`node.h:245-256`, `:418-787`) | Enumerations and declarations → table + prose. |
| 9.x | `NodeHidden` (`node.h:1475-1481`, indented block) | Class definition → prose. |
| 4.2 | Pseudocode of the render traversal (`rndrgn.cpp:7076-7130`) | An untagged block that mixed pseudocode with C++ syntax (`pNode->…`, `;`): neutralised into pure pseudocode in Spanish. |

### `03-render-engine.md` — 10 blocks

| § | Original content | Rationale |
|---|---|---|
| 1.2 | `struct GCONTEXT` (`gconsts.h:237`) | Struct definition **from a proprietary header** (CDraw, not GPL): replaced by its description (validation word + opaque block) and the sentinel value. |
| 1.x (h) | Signature of `GColour_SetTilePattern` (`gdraw.h:351`) | Literal signature from a proprietary header → parameter/type/role table, which additionally explains what each translation table is for. |
| 1.x (j) | Three `GDraw2_*` signatures (`gdraw2.h`) | Likewise → function/inputs/role table. |
| 2.1 | `struct GMATRIX` + `const INT32 FX` (`gconsts.h:310`, `:354`) | Struct definition → field/width/meaning table. |
| 2.1 | Construction of the matrix (`grndrgn.cpp:5297-5340`) | **Function body**: rewritten as a 3-step procedure, keeping the 2.30 fixed point, the 30 fractional bits and the sign change of the translation. |
| 2.6.2 | Four gradient-table `struct`s (`gconsts.h:248`, `:262`) | Struct definitions → structure/fields/use table. |
| 2.6.x | Ramp construction loop with a profile (`gradtbl.cpp:1206`) | **Function body**: neutral pseudocode. |
| 2.6.x | Fixed-point interpolation (`gradtbl.cpp:1562`) | **Function body**: neutral pseudocode, explaining that the added term is the rounding to +0.5. |
| 2.7 | `GRenderRegion::MapTranspTypeToGDraw` (`grndrgn.cpp:8844`) | **Function body**: rewritten as the rule it implements (a change of base between two contiguous numberings, in two stretches). |
| 2.8 | `PlasmaFractalFill::Adjust` (`fracfill.cpp:213-262`) | **Function body**: neutral pseudocode with the two components (noise and attraction) named. |

### `04-feature-inventory.md`

No code blocks, as expected. Only the clean-room notice was added.

### `05-technology-stack.md` and `06-xarast-format.md`

No C++ blocks. Everything they contain is our own material (TOML for our
workspace, CI YAML, `.xarast` XML, RELAX NG schemas, proposed Rust, console
output) or published third-party facts. Only the notice was added. In `05` an
update note about the licence was also added (see "ATTENTION").

---

## ATTENTION

### 1. Licence contradiction inside `05-technology-stack.md` — **resolved with a note, pending a formal decision**

Document `05` concludes, in its §1 and in its "DECISIONS" section, that
**Xarast must be released under GPL-3.0-or-later**, and even includes an
"IMMEDIATE ACTION: replace `/home/user/Xarast/LICENSE` (MIT today) with
GPL-3.0". That reasoning starts from the assumption that Xarast would be a
**derivative work** of Xara LX; under clean-room discipline that assumption does
not hold and the conclusion falls with it.

**What was done:** two notes were added (one at the start of §1 and another
above the decision block) marking the reasoning as obsolete and fixing
MIT OR Apache-2.0 as the decision in force. The analysis has **not** been
deleted, because the per-crate licence comparison remains useful.

**State in the repository:** during this pass, the project has already
formalised the decision outside `docs/research/`: `docs/11-licensing-and-clean-room.md`,
`LICENSE-MIT` and `LICENSE-APACHE` exist, and `CLAUDE.md` records
`MIT OR Apache-2.0` as the licence together with the hard clean-room rule. All
that remains is to review the `allow` list in `deny.toml` (proposed in `05` §1.4)
so that it reflects a permissive project rather than a GPL-3 one.

### 1 bis. The language of these documents versus the new rule in `CLAUDE.md`

`CLAUDE.md` now establishes that **the whole repository is written in English,
with no exceptions**, including file names (and `docs/` has in fact already been
renamed to English). The six documents in `docs/research/` and this report
remain in **Spanish**, with Spanish file names, because that is what this task
explicitly asked for and because their entire content is in Spanish. **This is
not a clean-room problem**, but it is an outstanding inconsistency: translating
the ~780 KB of `01`–`06` and renaming them is a separate task, and must be
planned as such. The clean-room notices that were added would have to be
translated along with them.

### 2. Residual risk: the density of the original's internal names

Documents `02` and `03` still cite **a great many class, member and method names
internal** to Xara LX (`NodeRenderableBounded`, `m_LastRequestedPixWidth`,
`MapTranspTypeToGDraw`…). This is **legitimate**: they are facts needed to read
the reference tree and to reason about behaviour, and API names are not, on
their own, protectable expression in the sense that matters here. But the
practical risk is worth keeping in mind:

* **Operational recommendation:** Xarast's identifiers **must not** be traced
  from the original's. Where the tables in this report use the original's name,
  it is as a *cross-reference*, not as a name to adopt. The Rust design in `02`
  §10 already uses its own nomenclature; keep to that line.

### 3. CDraw headers (`GDraw/*.h`): a **proprietary** licence, not GPL

The signatures and structures that `03` documented did not come from GPL code
but from the headers of the closed binary library `libCDraw.a`, whose licence
(`libs/LIBS-LICENSE`) is **more restrictive** than the GPL. Reproducing them
verbatim was the most delicate point in the whole set. None remain: all of them
have been turned into descriptive tables. **Rule to maintain:** never paste
content from `GDraw/*.h` into the documentation or the code again.

### 4. A verbatim quotation kept deliberately

`05` §1.1 reproduces three lines of the original's **licence notice** ("Xara LX
is free software…"). It is kept on purpose: it is documentary evidence that the
original is GPL-2.0-**only**, it is the text of a licence (not creative
expression of the program), and quoting it is the normal, expected use of a
licence notice. It is not considered a problem.

### 5. Test corpus

The `.xar` validation files cited (`xara-xtreme/testfiles/`, `Designs/`,
`Templates/`, `TextDesigns/`) are **not** code: they are test data from the
original tree, also covered by its licence. Using them locally to validate the
importer is legitimate; **they must not be redistributed inside the Xarast
repository**. The byte dumps that appear in `01` are minimal header fragments,
extracted with our own tool, and raise no problem.

---

## Permanent rule for future tasks

> In `docs/research/` and in any new document: **describe, do not transcribe**.
> If it is necessary to show how something in the original works, write it in
> English or in our own pseudocode/Rust, with the `file:line` reference so that
> whoever implements it can check it. Never paste C++ from the reference tree,
> and **never** content from `GDraw/*.h`.
