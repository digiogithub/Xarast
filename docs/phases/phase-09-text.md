# Phase 9 — Text

> After this phase Xarast has real typography: you can click on the canvas and
> type, the glyphs are shaped by HarfBuzz-class shaping with the document's
> fonts, the 14 text documents in the reference corpus render correctly, and
> any of it can be turned into editable curves.

## Goal

Build `xarast-text` and the text tool: a font database that finds the system's
fonts on Linux, Windows and macOS; a shaping and layout engine that reproduces
Xara's paragraph and character model; an on-canvas editing experience with a
caret, selection and live reflow; and a conversion to outlines that is exact.

The measure of success is the corpus: `/home/user/xara-xtreme/TextDesigns/`
contains **14 `.xar` files**, each built to exercise one text feature. They are
this phase's acceptance fixture. A feature is not done until its file renders.

## Scope

### In scope

**Font handling**

- System font enumeration and family/style matching through `fontique`
  (fontconfig on Linux, DirectWrite on Windows, CoreText on macOS), wrapped so
  that no other crate ever sees `fontique` types.
- Script-based font **fallback** for characters the selected face cannot cover.
- **Embedded fonts**: faces stored in the document, read from bytes with
  `skrifa`, taking priority over system faces of the same name.
- **Missing-font substitution**: when a document names a face that is not
  installed and not embedded, choose a substitute deterministically and record
  the substitution so the UI can report it and so a later install restores the
  original. (Xara used PANOSE matching, `Kernel/ccpanose.cpp`; our matching
  policy is decided in this phase — see W9.1.)
- A font cache keyed by `(family, weight, style, stretch)` with `Arc`-shared
  face data, and a background thumbnail generator for the font gallery.

**Text model (`xarast-doc`)**

- `TextStory` → `TextLine` → `TextItem` per `research/02 §10.11`, with
  `TextItem` as one enum replacing five node classes: `Char`, `Kern`, `Tab`,
  `LineBreak { paragraph }`.
- **The caret is not a node.** `CaretNode` and the static `pFocusStory` of the
  original are replaced by `TextCursor` living in the tool's state
  (`research/02 §10.11`, "Cambio 1").
- Lines are **formatted lines**, not paragraphs: word wrap restructures the line
  list, and `LineBreak { paragraph: true }` is the real paragraph boundary.
  Keeping this shape is what makes `.xar` import faithful and what makes
  `research/06 §6.7`'s `<tspan>`-per-line mapping direct.
- Cached per-line metrics (`LineMetrics`) holding the resolved attribute values,
  because resolving the attribute stack per character is too expensive to do
  during layout — this is the original's solution and it is the right one.

**Shaping and layout (`xarast-text`)**

- Shaping through `parley` (which uses `harfrust` for shaping, ICU4X for bidi
  and segmentation, `skrifa` for face data, `fontique` for fonts).
- **Bidi and complex scripts**: full UAX #9 bidi and script itemisation, with
  the `complex-scripts` feature enabled. Arabic, Hebrew, Indic and CJK must
  shape correctly; `hebrew.xar` is the gate.
- **OpenType features and variations** per style range (`liga`, `dlig`, `smcp`,
  `onum`, `tnum`, `kern`, `ss01`…), and variable-font axes.
- The three story modes of `research/02 §7.2`:
  - **point text** — `width == 0`, no path, lines grow freely;
  - **column text** — `width > 0`, word wrap to that width;
  - **text on a path** — each glyph placed along a curve (see W9.5).
- **Justification**: left, right, centre, full. Full justification distributes
  the slack across characters and spaces separately, as `FormatState`'s
  `ExtraOnChars` / `ExtraOnSpaces` do.
- **Line spacing**: absolute (millipoints) and proportional (ratio of the
  line's maximum font size).
- **Tracking**: thousandths of the em width (not millipoints, despite the
  original's declared type — `docs/memory/text.md`) added to every advance,
  with the last character's tracking excluded from the line width
  (`TextLine::GetLastCharTracking`).
- **Kerning**: automatic pair kerning from the font (on/off per story), plus
  **manual kerning** as `TextItem::Kern` between two characters.
- **Baseline shift**, and **super/subscript** as a baseline shift plus a size
  scale.
- **Glyph aspect ratio** (horizontal stretch).
- **Margins, first-line indent, and tab stops** with left / centre / right /
  decimal tab types.
- **Story-level character transforms**: scale, aspect, rotation, shear applied
  to characters *before* fitting them to a path (`CharsScale`, `CharsAspect`,
  `CharsRotation`, `CharsShear`).

**Text tool (`xarast-app`, `xarast-ui`)**

- Click to place a point-text caret; drag to define a column-text width; click
  an existing story to enter it.
- Caret movement: character, word, line, start/end of line, start/end of story,
  in **visual** order for bidi text (logical-order movement is a preference).
- Selection: shift-extend by any caret motion, double-click word, triple-click
  line, drag-select, and select-all within the story.
- Typing, deleting (backspace and forward delete, both grapheme-cluster aware),
  paste as text, and IME preedit display.
- Applying character and paragraph attributes to the selection.
- Text infobar: family, size, bold, italic, underline, justification, line
  spacing, tracking, and the OpenType feature panel.
- An interactive text ruler showing margins, first-line indent and tab stops
  while a story is being edited.
- Font gallery with per-family preview thumbnails generated in the background.

**Convert to shapes**

- `TextStory` → group of paths, via `skrifa::outline::OutlinePen` into
  `kurbo::BezPath`, preserving per-character attributes as fill/stroke on the
  produced paths, and preserving the source text for accessibility
  (`xarast:was-text`, `research/06 §6.7`).

**Round-trip**

- `.xar` import of tags 2100-2117, 2200-2204, 2900-2920 and the XaraLX
  extensions 4200-4207 into the model (phase 3 parses them; phase 9 is where
  they must become correct text).
- `.xarast` write and read per `research/06 §6.7`, including WOFF2 subset
  embedding, the `fsType` licence check, and the generic fallback chain.

### Explicitly out of scope (and which phase owns it)

| Not in this phase | Owner |
|---|---|
| **Linked/flowed text stories** (`TAG_TEXT_STORY_LINK_INFO` 4206) — text flowing from one frame into the next | **Phase 15**. The model reserves `story_next` / `story_prev` fields and the format round-trips them verbatim, but nothing reflows |
| **Text inside an arbitrary shape** (area text that is not a rectangle) | **Phase 15** |
| **Live effects on text**, text with shadows / bevels / contours | **Phase 13** |
| **Text on a moulded path**, text inside an envelope | **Phase 13** |
| **Brush and variable-width strokes on text outlines** | **Phase 13** |
| **RTF import, `.txt` import** | Post-v1.0 (P3 in `research/04 §5.1`); paste-as-plain-text is in scope |
| **Vertical writing modes** (CJK vertical, `writing-mode: vertical-rl`) | Post-v1.0. Explicitly not attempted; documents using it are laid out horizontally with a warning |
| **Spell checking, hyphenation dictionaries** | Post-v1.0. Hyphenation is not attempted; `FindBreakChar` is word-boundary only |
| **Font gallery web download, clipart-style font libraries** | Never (P3, dropped) |
| **PANOSE-based substitution using Xara's own tables** | The *mechanism* is in scope, the original's tables are not — clean-room rule. See W9.1 |

## Prerequisites

| Needs | From | Specifically |
|---|---|---|
| Node arena, attribute stack, command bus, undo | Phase 2 | architecture §3.6, §4 |
| Path rendering, fills, strokes | Phase 4 | glyph outlines are just paths |
| Canvas, viewport, overlay pass, keyboard focus routing | Phase 5 | |
| `.xar` parser reaching the text tags without error | Phase 3 | |
| `xarast-geom`: `BezPath`, arc-length parametrisation (`kurbo::ParamCurveArclen`) | Phase 1 | needed for text on a path |
| Tool framework and infobar host | Phase 7 | |
| Resource table with `TypefaceId` | Phase 2 | `research/02 §10.12` |
| `.xarast` resource directory for `resources/fonts/` | Phase 6 | |

Phase 9 runs **in parallel with phase 8** (roadmap). They touch different
crates; the only shared surface is `xarast-doc`'s attribute enum, whose text
variants are agreed before both start.

## Workstreams

### W9.1 — Font database and system enumeration

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.1.1 | `FontDb` wrapping `fontique`: enumerate families, query by (family, weight, style, stretch) | `xarast-text` | M | — |
| T9.1.2 | Face loading and `Arc`-shared face data with an LRU cache | `xarast-text` | M | T9.1.1 |
| T9.1.3 | Script-based fallback chain, per platform, with a preference override | `xarast-text` | M | T9.1.1 |
| T9.1.4 | Embedded-face registration from `&[u8]`, shadowing system faces | `xarast-text` | S | T9.1.2 |
| T9.1.5 | Missing-font substitution policy + `FontSubstitution` report | `xarast-text` | M | T9.1.3 |
| T9.1.6 | Background enumeration so startup is not blocked | `xarast-text` | S | T9.1.1 |
| T9.1.7 | Font gallery model and background preview rendering | `xarast-app`, `xarast-ui` | M | T9.1.2 |
| T9.1.8 | Platform smoke tests: Linux (fontconfig), Windows (DirectWrite), macOS (CoreText) | `xarast-text` | M | T9.1.1 |

**Substitution policy is decided in this phase, not inherited.** Xara used
PANOSE classification; we cannot copy its tables. The policy this phase
implements and measures:

1. Exact family-name match (case-insensitive, whitespace-normalised).
2. Family-name match after stripping style suffixes ("Arial Bold" → "Arial"
   + weight 700).
3. A small built-in alias table for the handful of metric-compatible families
   that matter for a 2000s-era corpus (Arial↔Liberation Sans↔Helvetica,
   Times New Roman↔Liberation Serif↔Times, Courier New↔Liberation Mono).
   This table is **written by us from public metric-compatibility facts**, not
   transcribed from anything.
4. `fontique`'s generic-family fallback (`sans-serif` / `serif` / `monospace`)
   chosen from the missing face's OS/2 classification if it can be read, and
   from the name otherwise.
5. Last resort: the platform default UI font.

The chosen substitute is recorded per `TypefaceId` and surfaced in the UI. It is
**never** written back into the document — the document keeps the original
family name, so installing the font later restores it.

**Enumeration must not block the 400 ms cold-start budget.** Enumerate on the
I/O thread; the text tool is unusable until it completes, which is fine because
the window is already up. The font gallery shows a spinner.

### W9.2 — Text model in the document

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.2.1 | `NodeKind::TextStory`, `::TextLine`, and the `TextItem` list | `xarast-doc` | M | — |
| T9.2.2 | Text attribute variants on `AttrValue` (typeface, size, bold, italic, underline, justification, line spacing, tracking, baseline, script, aspect, margins, indent, ruler) | `xarast-doc` | M | T9.2.1 |
| T9.2.3 | `TextPos` and `TextCursor` types; conversion to/from a byte offset in the story's logical text | `xarast-doc` | M | T9.2.1 |
| T9.2.4 | Edit commands: `InsertText`, `DeleteRange`, `SplitLine`, `MergeLines`, `SetTextAttr`, `InsertKern`, `SetStoryMode` | `xarast-doc` | L | T9.2.3 |
| T9.2.5 | Story invariants: every line ends with a break item or is the last; no empty story without one line; ramps of attributes never straddle a line boundary incorrectly | `xarast-doc` | M | T9.2.1 |
| T9.2.6 | `.xar` import mapping: tags 2100-2117, 2200-2204, 2900-2920, 4200-4207 | `xarast-xar` | L | T9.2.2 |

**Word wrap restructures the tree, and that fights undo.** In the original,
`TextLine::Wrap` moves character nodes between lines. If every moved character
becomes an undo action, typing one character in the middle of a paragraph
produces hundreds of actions. The rule this phase fixes: **a text edit command's
inverse restores the story's item list, not the line structure.** Lines are
derived state. `DeleteRange`/`InsertText` store the affected paragraph's item
run before and after; reflow is recomputed on apply and on undo. This is
`SetKind`-on-the-paragraph semantics, and it makes the undo entry proportional
to the paragraph, not to the document. Record the paragraph size threshold above
which we switch to a diff (proposal: 4 KiB of items) in `docs/memory/text.md`.

### W9.3 — Shaping and layout

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.3.1 | `Shaper` façade over `parley`: style ranges in, positioned glyph runs out | `xarast-text` | L | T9.1.2 |
| T9.3.2 | `FontMetrics` implementation (`research/02 §10.11`) for the doc layer | `xarast-text` | S | T9.3.1 |
| T9.3.3 | Line breaking for column text; `FindBreakChar` equivalent (word boundaries via ICU4X segmentation) | `xarast-text` | M | T9.3.1 |
| T9.3.4 | Justification: left/right/centre/full with separate char and space slack | `xarast-text` | M | T9.3.3 |
| T9.3.5 | Line metrics: ascent, descent, size, absolute and ratio line spacing | `xarast-text` | M | T9.3.1 |
| T9.3.6 | Tracking, manual kern items, auto-kern on/off | `xarast-text` | M | T9.3.1 |
| T9.3.7 | Baseline shift, super/subscript, aspect ratio | `xarast-text` | S | T9.3.5 |
| T9.3.8 | Tab stops (left/centre/right/decimal) and the ruler model | `xarast-text` | M | T9.3.3 |
| T9.3.9 | Bidi: logical↔visual mapping, mixed-direction runs, neutral resolution | `xarast-text` | M | T9.3.1 |
| T9.3.10 | OpenType feature and variation plumbing per style range | `xarast-text` | M | T9.3.1 |
| T9.3.11 | Layout cache keyed by `(content hash, style hash, width, mode)` | `xarast-text` | M | T9.3.1 |
| T9.3.12 | `format_story` writing positions back into the arena | `xarast-doc` | M | T9.3.1, T9.2.4 |

**Millipoints meet floats here, and that is the precision hazard of this phase.**
`parley` works in `f32` pixels. The document stores millipoints (`i32`). The
rule (architecture §3.4, `research/03 §3.7`): shape at a **fixed nominal size**
in `f32`, and convert advances back to millipoints by
`round_half_away_from_zero(advance_f32 * size_mp / nominal)`. Never shape at the
document's actual size and never accumulate positions in `f32` across a long
line — accumulate the millipoint values. A 200-character line at 10 pt shaped
naively in `f32` and accumulated drifts visibly by the end.

**Manual kerning applies after shaping, never before.** `parley` returns
positions with OpenType kerning already applied; a `TextItem::Kern` is an
additional millipoint delta inserted into the advance stream at that cluster
boundary. It must survive re-shaping, so it is anchored to the item index, not
to a glyph index.

**Tracking is excluded from the last character.** This is not a detail: it is
what makes centred and right-aligned text align correctly. `TextLine`'s width
is `sum(advances) - tracking_of_last_char`.

### W9.4 — The text tool

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.4.1 | Tool state machine: place caret / drag column / enter story / select | `xarast-app` | L | T9.2.3 |
| T9.4.2 | Caret rendering (blinking, bidi-correct, split caret at direction boundaries) | `xarast-ui` | M | T9.3.9 |
| T9.4.3 | Selection rendering: per-line rectangles in visual order | `xarast-ui` | M | T9.3.9 |
| T9.4.4 | Keyboard navigation incl. visual-order arrow keys, word/line/story jumps | `xarast-app` | M | T9.3.9 |
| T9.4.5 | Mouse: click-to-position (nearest cluster boundary), drag, double/triple click | `xarast-app` | M | T9.3.1 |
| T9.4.6 | Typing, grapheme-cluster-aware deletion, undo grouping per typing burst | `xarast-app` | M | T9.2.4 |
| T9.4.7 | IME preedit: display, commit, cancel | `xarast-app`, `xarast-shell` | M | T9.4.6 |
| T9.4.8 | Clipboard: paste as text, copy with attributes inside Xarast | `xarast-app` | M | T9.4.6 |
| T9.4.9 | Text infobar and OpenType feature panel | `xarast-ui` | M | T9.3.10 |
| T9.4.10 | Interactive text ruler with draggable margins, indent and tab stops | `xarast-ui` | L | T9.3.8 |

Typing produces one undo step per burst: consecutive `InsertText` commands with
the same `CoalesceKey` (the story id) and less than 500 ms apart merge, and any
caret move, attribute change or other command ends the burst. Same mechanism as
phase 8's drag coalescing — one implementation, in the history.

### W9.5 — Text on a path (architecture open question 4)

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.5.1 | Spike A: post-transform — lay out normally, then map each cluster onto the path by arc length | `xarast-text` | M | T9.3.1 |
| T9.5.2 | Spike B: layout-aware — feed per-position available advance back into breaking and justification | `xarast-text` | L | T9.3.3 |
| T9.5.3 | Fidelity measurement of both against the corpus fixtures | `xarast-text` | M | T9.5.1, T9.5.2 |
| T9.5.4 | Implement the winner; delete the loser | `xarast-text` | M | T9.5.3 |
| T9.5.5 | Path as an editable child node; reverse-along-path; start offset; tangential vs upright characters | `xarast-doc` | M | T9.5.4 |
| T9.5.6 | `.xarast` mapping to `<textPath>` + `xarast:text-path` extension | `xarast-format` | S | T9.5.4 |

**How open question 4 gets decided, concretely.** The architecture asks whether
text on a path needs our own layout pass over `parley` or whether `parley`'s
output can simply be transformed. The decision is made by measurement, not
opinion, and it is made in T9.5.3:

- **Fixtures**: `TextDesigns/AngledText.xar`, `TextDesigns/Rotated.xar`,
  `Designs/TextCurve.xar`, plus two synthetic cases — a high-curvature path
  (letters must not collide on the inside of a tight curve) and a closed path
  with full justification.
- **Metric**: per-glyph origin displacement against the reference render,
  expressed in fractions of that glyph's advance. **Pass = 95th percentile
  ≤ 0.25 advance and maximum ≤ 0.5 advance.**
- **Rule**: if spike A passes on all five fixtures, spike A wins — it is a few
  hundred lines over `kurbo::ParamCurveArclen` and it keeps us on `parley`'s
  fast path. If it fails only on the closed-path-with-justification case,
  spike A wins with justification on a closed path listed as a known limitation
  for phase 15. If it fails on the curvature cases, spike B wins and we own a
  layout pass.
- The outcome, the numbers and the date go in `docs/memory/text.md` and the
  architecture's open-questions table is updated to say "decided in phase 9",
  with the answer.

Note that architecture §7 lists this question as decided by "Phase 7". That was
written before the roadmap settled text into phase 9; the substance is
unchanged, only the phase number. Fix the table when you close this phase.

**Outcome (2026-09-24, XARA-US-0048): spike A won on all five fixtures**
(worst p95 0.0021 of an advance; spike B up to 5.8 advances off), and the
architecture table says so. T9.5.1–T9.5.4 are done; spike B lives only as the
measurement's comparator. T9.5.5 and T9.5.6 remain. Details in
`docs/memory/text.md`, "Text on a path".

### W9.6 — Convert to shapes

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.6.1 | Glyph outline extraction: `skrifa` `OutlinePen` → `kurbo::BezPath`, with variation coordinates applied | `xarast-text` | M | T9.1.2 |
| T9.6.2 | Outline cache keyed by `(face, glyph, variation coords)`; scaling is a transform, not a re-extraction | `xarast-text` | S | T9.6.1 |
| T9.6.3 | `ConvertTextToShapes` command producing a group of paths with the right attributes | `xarast-doc` | M | T9.6.1 |
| T9.6.4 | Preserve the source text on the produced group for accessibility and search | `xarast-doc` | S | T9.6.3 |
| T9.6.5 | Same path used by `.xarast` conformance profile C and by PDF/SVG export when a font cannot be embedded | `xarast-format` | S | T9.6.1 |

Outlines come out in font units and are scaled by `size / units_per_em`. Extract
once per glyph at unit scale and cache; a 3,000-glyph story converted at
several sizes must not re-extract.

### W9.7 — Corpus, round-trip and golden images

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T9.7.1 | `xar-dump --tags` over all 14 `TextDesigns/` files; record the actual tag inventory per file | `xarast-cli` | S | phase 3 |
| T9.7.2 | Golden render of each file through the CPU backend, with fonts pinned | `xarast-cli` | M | T9.3.12 |
| T9.7.3 | `.xar` → `.xarast` → reload → render comparison for all 14 | `xarast-format` | M | T9.7.2 |
| T9.7.4 | WOFF2 subsetting of embedded faces, with the `fsType` licence check | `xarast-format` | M | T9.1.4 |
| T9.7.5 | Multi-script test documents authored in Xarast (Arabic, Devanagari, Thai, CJK) | — | S | T9.3.9 |

**Fonts must be pinned for golden images.** A render test that depends on the CI
machine's font set is not a test. Vendor a small set of permissively licensed
faces (DejaVu, Liberation, Noto Sans Arabic / Devanagari / Thai / CJK subset)
under `tests/fonts/`, and run golden renders with the system font path
overridden to that directory only. Record this in `docs/memory/text.md`.

### The acceptance fixture: `/home/user/xara-xtreme/TextDesigns/`

Fourteen files. Each is a pass/fail gate for one part of this phase. The
"exercises" column is the expected coverage; T9.7.1 confirms it against the
actual tag inventory of each file, and any file that turns out to exercise
something else has its row corrected rather than the test weakened.

| # | File | Size | Exercises | Gates workstream |
|---|---|---|---|---|
| 1 | `SimpleText.xar` | 11.8 KB | Point text: a single story with `width == 0`, default attributes. The smoke test — if this is wrong, nothing else matters | W9.3 |
| 2 | `FontChangesInText.xar` | 9.2 KB | Typeface and size changing **within** a story: attribute scoping across `TextChar` runs, multiple faces in one line, per-run shaping boundaries | W9.2, W9.3 |
| 3 | `Paragraph.xar` | 21.0 KB | Column text: `width > 0`, word wrap, the distinction between a wrap break and a real paragraph break (`EOLNode`) | W9.3 |
| 4 | `TextJust.xar` | 143.5 KB | All four justifications including **full**: slack distribution across characters and spaces. The largest of the small files — many lines, so distribution errors are visible | W9.3 |
| 5 | `LineSpacing.xar` | 21.1 KB | Absolute (millipoint) and proportional (ratio) line spacing, and their interaction with mixed font sizes on one line | W9.3 |
| 6 | `Tracking.xar` | 20.5 KB | `AttrTxtTracking`: millipoints added to every advance, and the exclusion of the last character's tracking from the line width | W9.3 |
| 7 | `Kerning.xar` | 12.2 KB | **Automatic** pair kerning from the font's tables, and the auto-kern on/off flag | W9.3 |
| 8 | `ManualKern.xar` | 20.5 KB | **Manual** kerning: `KernCode` items between characters, surviving re-shaping | W9.3 |
| 9 | `BaselineShift.xar` | 20.5 KB | `AttrTxtBaseLine`: explicit baseline offsets, independent of script | W9.3 |
| 10 | `SuperSub.xar` | 30.3 KB | `AttrTxtScript`: superscript and subscript as baseline shift plus size scale | W9.3 |
| 11 | `AngledText.xar` | 41.7 KB | Story-level character transforms: rotation and shear applied to characters (`CharsRotation`, `CharsShear`) | W9.3, W9.5 |
| 12 | `Rotated.xar` | 241.0 KB | The whole story transformed by `StoryMatrix`, at volume. Also the phase's **performance** fixture: it is 20× the size of the others | W9.3, perf |
| 13 | `embeddedFonts.xar` | 3.9 KB | Fonts carried inside the document: the embedded-face path, and what happens when the face is also installed | W9.1 |
| 14 | `hebrew.xar` | 12.4 KB | RTL text: bidi resolution, visual-order caret movement, right-aligned defaults, mixed-direction runs | W9.3, W9.4 |

**T9.7.1 inventory (2026-09-24, XARA-T-0260).** Confirmed against
`xar-dump --tags`; the per-file table is in `docs/memory/text.md`
("TextDesigns acceptance gate"). Corrections to the column above:
`Kerning.xar` also carries manual kerns (`TAG_TEXT_KERN`), its automatic
kerning being the story's flag; `AngledText.xar` also sets bold, italic
and tracking; `LineSpacing.xar` uses only proportional spacing (no
absolute line-spacing tag occurs in any of the 14); `Rotated.xar` covers
every justification, super/subscript, tracking and kerns;
`embeddedFonts.xar` carries no font data, so criterion 5 needs a
synthetic document.

`Designs/TextCurve.xar` is the fifteenth fixture, from the other corpus
directory: it is the text-on-a-path gate for W9.5.

## Public API introduced

```rust
// ─────────────────────────────── xarast-text ────────────────────────────────

slotmap::new_key_type! { pub struct FaceId; }

/// System + embedded font database. Wraps `fontique`; nothing outside this
/// crate sees a fontique type.
pub struct FontDb { /* … */ }

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FontQuery {
    pub family: Arc<str>,
    pub weight: u16,          // 100..=900
    pub style: FontStyle,     // Normal | Italic | Oblique(f32)
    pub stretch: u16,         // 50..=200, percent
}

#[derive(Clone, Debug)]
pub struct FontMatch {
    pub face: FaceId,
    /// `None` when the query matched exactly.
    pub substituted_for: Option<Arc<str>>,
    pub embedded: bool,
}

impl FontDb {
    pub fn new_system() -> Self;
    /// Runs enumeration on the calling thread; call it off the main thread.
    pub fn load_system_fonts(&mut self) -> Result<usize, FontError>;
    pub fn register_embedded(&mut self, name: &str, data: Arc<[u8]>)
        -> Result<FaceId, FontError>;
    pub fn query(&self, q: &FontQuery) -> FontMatch;
    /// Fallback chain for a character the primary face cannot cover.
    pub fn fallback_for(&self, c: char, base: &FontQuery) -> Option<FaceId>;
    pub fn families(&self) -> impl Iterator<Item = &str>;
    pub fn face_data(&self, id: FaceId) -> Arc<[u8]>;
    /// True when the face's OS/2 `fsType` forbids embedding (research/06 §6.7).
    pub fn embedding_denied(&self, id: FaceId) -> bool;
}

/// A contiguous run of text sharing every style property.
#[derive(Clone, PartialEq, Debug)]
pub struct StyleRange {
    pub range: Range<usize>,           // byte range in the story's logical text
    pub font: FontQuery,
    pub size: Millipoints,
    pub tracking: i32,                 // thousandths of an em (as stored)
    pub baseline_shift: Millipoints,
    pub script: TextScript,            // Normal | Super | Sub
    pub aspect: f32,                   // 1.0 = unstretched
    pub features: Arc<[FontFeature]>,  // ("liga", 1), ("smcp", 1), …
    pub variations: Arc<[FontVariation]>,
    pub underline: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FontFeature { pub tag: [u8; 4], pub value: u32 }
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FontVariation { pub tag: [u8; 4], pub value: f32 }

/// Everything layout needs that is not per-run.
#[derive(Clone, PartialEq, Debug)]
pub struct ParagraphStyle {
    pub justification: Justification,       // Left | Right | Centre | Full
    pub line_spacing: LineSpacing,          // Absolute(mp) | Ratio(f32)
    pub left_margin: Millipoints,
    pub right_margin: Millipoints,
    pub first_indent: Millipoints,
    pub tabs: Arc<[TabStop]>,
    pub auto_kern: bool,
    pub base_direction: Direction,          // Ltr | Rtl | Auto
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TabStop { pub pos: Millipoints, pub kind: TabKind }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabKind { Left, Centre, Right, Decimal }

/// How the story is laid out. == the three modes of research/02 §7.2.
#[derive(Clone, PartialEq, Debug)]
pub enum StoryMode {
    /// `StoryWidth == 0`: lines grow freely.
    Point,
    /// `StoryWidth > 0`: wrap at this width.
    Column { width: Millipoints },
    /// Flow along a path.
    OnPath { path: NodeId, start_offset: Millipoints,
             reversed: bool, tangential: bool },
}

/// The layout engine. Pure: takes text and styles, returns geometry.
pub struct Shaper { /* … */ }

impl Shaper {
    pub fn new(fonts: Arc<FontDb>) -> Self;
    pub fn layout(&self, text: &str, runs: &[StyleRange],
                  para: &ParagraphStyle, mode: &StoryMode,
                  path: Option<&BezPath>) -> Layout;
    /// Metrics without geometry — the replacement for `FormatRegion`.
    pub fn measure(&self, text: &str, runs: &[StyleRange]) -> RunMetrics;
}

/// The result of laying out one story.
pub struct Layout {
    pub lines: Vec<LaidLine>,
    pub bounds: Rect,
    pub substitutions: Vec<FontSubstitution>,
}

pub struct LaidLine {
    pub glyphs: Vec<PlacedGlyph>,
    pub ascent: Millipoints,
    pub descent: Millipoints,
    pub baseline_y: Millipoints,
    pub logical_range: Range<usize>,
    /// Visual-order runs, for caret movement and selection rectangles.
    pub bidi_runs: Vec<BidiRun>,
}

pub struct PlacedGlyph {
    pub face: FaceId,
    pub glyph_id: u16,
    /// Full placement: position, and rotation when the glyph sits on a path.
    pub transform: Matrix,
    pub advance: Millipoints,
    /// Byte offset of the cluster this glyph belongs to.
    pub cluster: usize,
}

#[derive(Clone, Debug)]
pub struct FontSubstitution { pub requested: Arc<str>, pub used: Arc<str>, pub face: FaceId }

/// Glyph outlines, in font units, cached. Scale by `size / units_per_em`.
pub fn glyph_outline(fonts: &FontDb, face: FaceId, glyph: u16,
                     variations: &[FontVariation]) -> Option<Arc<BezPath>>;

// ──────────────────────────────── xarast-doc ────────────────────────────────

/// A position in a story: which line, and which item within it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct TextPos { pub line: NodeId, pub item: u32 }

/// Caret and selection. Lives in the tool, NOT in the document tree
/// (research/02 §10.11, "Cambio 1").
#[derive(Clone, Copy, Debug)]
pub struct TextCursor { pub story: NodeId, pub anchor: TextPos, pub head: TextPos }

impl TextCursor {
    pub fn is_collapsed(&self) -> bool;
    pub fn ordered(&self) -> (TextPos, TextPos);
}

/// Commands.
pub struct InsertText   { pub story: NodeId, pub at: TextPos, pub text: String }
pub struct DeleteRange  { pub story: NodeId, pub from: TextPos, pub to: TextPos }
pub struct SetTextAttr  { pub story: NodeId, pub from: TextPos, pub to: TextPos,
                          pub attr: TextAttr }
pub struct InsertKern   { pub story: NodeId, pub at: TextPos, pub amount: Millipoints }
pub struct SetStoryMode { pub story: NodeId, pub mode: StoryMode }
pub struct FitTextToPath { pub story: NodeId, pub path: NodeId }
pub struct ReverseStoryPath { pub story: NodeId }
pub struct ConvertTextToShapes { pub story: NodeId }

/// Re-layout a story into the arena. The one entry point; nothing else writes
/// `Placement` or `LineMetrics`. == TextStory::FormatAndChildren.
pub fn format_story(tree: &mut Tree, story: NodeId, shaper: &Shaper)
    -> Result<(), TextError>;

// ──────────────────────────────── xarast-app ────────────────────────────────

pub struct TextTool { /* … */ }

/// Caret motion, in the unit the user asked for. Visual vs logical matters
/// only for Character and only in bidi text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaretMotion {
    Character { visual: bool }, Word, Line, LineStart, LineEnd,
    StoryStart, StoryEnd, PageUp, PageDown,
}

pub fn move_caret(tree: &Tree, layout: &Layout, cur: TextPos,
                  motion: CaretMotion, forward: bool) -> TextPos;

/// Device-space caret geometry. Two rectangles at a direction boundary.
pub fn caret_rects(layout: &Layout, pos: TextPos) -> SmallVec<[Rect; 2]>;

/// Selection highlight, one rectangle per line run, in visual order.
pub fn selection_rects(layout: &Layout, cur: &TextCursor) -> Vec<Rect>;

/// Nearest insertion point to a document-space point.
pub fn hit_text(layout: &Layout, p: Point) -> TextPos;
```

## Acceptance criteria

1. `cargo test -p xarast-text` passes, including shaping golden tests: for each
   of the pinned test faces, a fixed string shapes to a fixed glyph-id and
   advance sequence.
2. `xarast-cli render --backend cpu` produces a PNG for **all 14** files in
   `TextDesigns/` with **zero errors and zero missing-glyph boxes**, and each
   matches its golden image within the phase-3 perceptual gate.
3. `xar-dump --tags TextDesigns/*.xar` reports **no unhandled text tag** in the
   ranges 2100-2117, 2200-2204, 2900-2920, 4200-4207.
4. `hebrew.xar` renders with correct RTL ordering, verified by asserting that
   the leftmost glyph of the first line has the **highest** logical cluster
   index of that line.
5. `embeddedFonts.xar` renders with the embedded face even when a system face of
   the same family is installed: asserted by comparing the rendered output
   against a run with the system face removed.
6. `TextJust.xar`: for every fully justified line except the last of each
   paragraph, the measured line width equals the column width to within 1
   millipoint.
7. `Tracking.xar`: the measured width of a right-aligned tracked line equals
   `sum(advances) - last_tracking`; a unit test compares against a hand-computed
   value.
8. `ManualKern.xar`: re-shaping the story (change the font size, then change it
   back) leaves every `TextItem::Kern` amount and position unchanged.
9. Convert-to-shapes on each of the 14 files produces a group whose rendered
   output differs from the text rendering by **no pixel outside antialiasing
   tolerance**, and whose bounding box matches the text ink bounds to within 1
   millipoint.
10. Text-on-a-path spike measurement (T9.5.3) is recorded with actual numbers
    for both spikes on all five fixtures, and architecture open question 4 is
    answered in `docs/memory/text.md`.
11. `Designs/TextCurve.xar` renders within the perceptual gate.
12. `.xar` → `.xarast` → reload → render for all 14 files: zero differing pixels
    against the direct-import render.
13. The `.xarast` produced from a document using a face whose `fsType` forbids
    embedding contains **no** font file for that face and carries
    `xarast:font-embed="denied"`. Asserted by unzipping and grepping.
14. Typing 200 characters produces **one** undo step; a single `Ctrl+Z` removes
    all 200.
15. A synthetic document mixing Latin, Arabic, Devanagari, Thai and CJK renders
    with no `.notdef` glyphs (asserted by scanning the `PlacedGlyph` stream for
    glyph id 0).
16. `cargo clippy --workspace -- -D warnings` and `cargo deny check licenses`
    pass.

## Performance budgets

| Budget | Target | How measured |
|---|---|---|
| System font enumeration (≈ 500 families) | ≤ 300 ms, off the main thread, not counted against cold start | `criterion` + startup trace |
| Shape + lay out a 1,000-glyph story, cold | ≤ 8 ms | `criterion` |
| Shape + lay out a 10,000-glyph story, cold | ≤ 60 ms | `criterion` |
| Re-layout after a one-character insertion (paragraph-scoped) | ≤ 3 ms | `criterion` |
| Layout cache hit | ≤ 1 µs | `criterion` |
| Caret movement (any motion) | ≤ 1 ms | `criterion` |
| Open and first paint of `TextDesigns/Rotated.xar` (241 KB) | ≤ 500 ms | CLI timing, matches the roadmap's `.xar` budget |
| Glyph outline extraction, cached | ≤ 500 ns per glyph | `criterion` |
| Convert a 3,000-glyph story to shapes | ≤ 150 ms | `criterion` |
| Font gallery: 500 family previews | ≤ 2 s total, fully off the main thread, UI never blocks > 4 ms | trace |

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `parley` is pre-1.0 and breaks API (`research/05 §5.4`) | High | Low | Everything `parley` goes behind `xarast-text`'s `Shaper`. Nothing else in the workspace names a `parley` type. Linebender's breakages are mechanical |
| Xara's layout differs from `parley`'s in ways that make corpus files not match | Medium | **High** | This is the real risk of the phase. Mitigation: measure per-glyph displacement, not whole-image difference, so a mismatch says *which* rule is wrong. Attack order: advances → tracking → line spacing → justification → wrap points. Budget a full workstream's worth of slack for it |
| Millipoint↔`f32` drift accumulates across a long line | Medium | Medium | Fixed rule in W9.3: shape at nominal size, accumulate in millipoints, round half away from zero. Regression test: a 500-character line's end position computed two ways must agree exactly |
| CJK/Arabic fallback is imperfect on Linux (`research/05 §5.4`) | Medium | Medium | Pinned test fonts in CI remove the variability; a preference lets a user force a fallback chain; a document records which faces were substituted |
| Word wrap restructuring the tree makes undo entries enormous | Medium | Medium | Paragraph-scoped inverses (W9.2). Threshold for switching to a diff recorded in memory |
| IME on Wayland behaves differently across compositors | Medium | Medium | Route preedit through `winit`'s IME events only; test on at least two compositors (one wlroots-based, one GNOME). Failures degrade to "no preedit display, commit still works", which is usable |
| Text on a path needs our own layout pass and blows the schedule | Medium | Medium | The spike is time-boxed: spike A first, and it only escalates to spike B if the measurement in T9.5.3 fails. Spike B's cost is known up front (`L`) |
| WOFF2 subsetting adds a dependency with a C toolchain | Low | Medium | Prefer a pure-Rust subsetter; if none is acceptable, fall back to embedding the full face (larger files, no C), and record the decision. Never add a C dependency to the AppImage for this |
| Hebrew/Arabic caret behaviour is subtly wrong and nobody on the team can tell | Medium | Low | Write the caret tests as assertions on cluster indices, not on appearance, so they are checkable without reading the script |

## Test plan

**Unit.** Font query and substitution ladder (each of the five steps, with a
synthetic font set). Shaping golden sequences against pinned faces. Line
breaking at known break points. Justification slack arithmetic. Tracking
exclusion. Tab stop resolution for all four kinds. Baseline shift and script
scaling. Bidi level resolution against a slice of the Unicode BidiTest data.

**Property.** `hit_text(layout, position_of(pos)) == pos` for every position in
a randomly generated story. `move_caret` forward `n` times then backward `n`
times returns to the start. Insert-then-delete of random text returns the story
to its original item list. Layout is deterministic: the same inputs give
byte-identical `Layout` twice.

**Corpus.** The 14 `TextDesigns/` files as golden renders plus round-trip, per
the acceptance criteria. `Designs/TextCurve.xar` for text on a path.

**Golden images.** One per corpus file, plus a feature matrix sheet: each
justification × each line-spacing mode × each script mode, rendered at 100 %
and 400 %.

**GPU/CPU parity.** Every text golden image rendered on both backends; `Final`
must match bit for bit.

**Fuzz.** `cargo-fuzz` on `Shaper::layout` with random Unicode strings
(including lone surrogates rejected at the boundary, unassigned code points,
long combining sequences, and 10,000-character runs of a single combining mark),
asserting no panic, no unbounded allocation and termination within a time limit.
A text shaper fed a pathological combining sequence is a denial-of-service
surface; treat it as one.

**Interaction.** Scripted command sequences: type / select / apply attribute /
undo / redo, asserting story contents and undo depth at each step. Bidi caret
walk over `hebrew.xar` asserting the visual order of visited positions.

**Platform.** T9.1.8 runs the font-enumeration smoke test on all three
platforms in CI. Linux is the gate for this phase; Windows and macOS results are
recorded but do not block (phase 14 owns them).

## Memory note

Create/update **`docs/memory/text.md`** with:

- **The answer to architecture open question 4**: which text-on-a-path approach
  won, the measured numbers for both spikes on all five fixtures, and the
  limitations accepted. Then correct architecture §7's table (it says "Phase 7";
  the phase is 9).
- The millipoint↔`f32` rule: shape at nominal size, accumulate in millipoints,
  round half away from zero — and the regression test that guards it.
- The font substitution ladder as implemented, and the alias table's contents
  and provenance (written by us, from public metric-compatibility facts).
- The paragraph-scoped undo rule for text edits and the size threshold at which
  it switches to a diff.
- The pinned test font set under `tests/fonts/` and why golden renders must
  never use system fonts.
- Which of the 14 corpus files were hardest to match and what the actual
  discrepancy turned out to be — this is the most valuable thing this phase can
  leave behind.
- Dead ends: layout approaches tried and abandoned; `parley` APIs that looked
  right and were not.

Update **`docs/memory/ui.md`** with the text tool state machine, the typing
coalescing rule, IME behaviour observed per compositor, and the text ruler's
interaction model.

Update **`docs/memory/xarast-format.md`** with the font embedding rules actually
implemented (subsetting choice, `fsType` handling, fallback chain emitted).
