# text

Memory note for **text**: fonts, shaping, layout, glyph outlines, and the
contract with the document model. Phase 9 (`docs/phases/phase-09-text.md`).

## Current state

Round 1 of phase 9 (2026-09-23) built `xarast-text` in isolation. Round 2
(the same day, text integration) made **imported text render**: the text
model in `xarast-doc` (W9.2, read side), the attribute bridge and the
walker's text path in `xarast-app`, font service and substitution reporting.
`text_pending` is 0 on the whole corpus; only `Designs/TextCurve.xar` is
approximate (text on a path drawn straight, `text_on_path_pending`). Edit
commands (T9.2.4), `format_story` writing lines back (T9.3.12), the text
tool (W9.4) and text on a path (W9.5) are later rounds.

| Story | Tasks | State |
|---|---|---|
| XARA-US-0044 W9.1 font database | T9.1.1–T9.1.4 done; T9.1.5 (substitution ladder) implemented and tested too | in review |
| XARA-US-0046 W9.3 shaping and layout | T9.3.1–T9.3.4 done; T9.3.5 (line metrics), T9.3.6 (tracking, manual kerns, auto-kern), T9.3.7 (baseline, script, aspect) and a first T9.3.9 (bidi) came along because layout cannot return lines without them | in review |
| XARA-US-0049 W9.6 outlines | T9.6.1–T9.6.2 done; T9.6.3–T9.6.5 are `xarast-doc`/`xarast-format` work | in progress |
| XARA-US-0045 W9.2 text model | read side done: `StoryText`, `TextPos`/`TextCursor`, attribute bridge, importer scoping and surrogates; edit commands (T9.2.4): `InsertText`, `DeleteRange` done (XARA-T-0223), `SetTextAttr`, `InsertKern`, `SetStoryMode` open; story invariants (T9.2.5) open | in review |
| XARA-US-0047 W9.4 text tool | T9.4.1 state machine, T9.4.2 caret (blinking, split at direction boundaries), T9.4.3 selection spans in visual order, T9.4.4 keyboard navigation done; the T9.4.5 mouse gestures (click-to-position, drag, double/triple click) came along. T9.4.6 typing, grapheme deletion, undo per burst done (XARA-T-0223, with the T9.2.4 `InsertText`/`DeleteRange` commands). IME, clipboard, infobar, ruler (T9.4.7–T9.4.10) open | in review |
| XARA-US-0050 W9.7 corpus | text renders in the walker (every corpus story); the `.xarast` text writer is exact (XARA-T-0172, done: 59/59 render round trip); golden images open | in progress |

Public API (`crates/xarast-text/src/lib.rs`):

- **`FontDb`** (`font/mod.rs`) — `&self` everywhere, state behind one mutex,
  `Arc`-shareable. `new_system()` (enumeration deferred) /
  `load_system_fonts()`, `new_isolated()` (no system fonts; tests and golden
  renders), `with_options()`. `families()`, `query()` /
  `query_with_panose()` → `FontMatch { face, family, substitution, embedded,
  synthesis }`, `fallback_for(c, base)`, `set_fallback_preference(script,
  families)`, `set_generic_families()`, `register_embedded(name, bytes)`,
  `face_data()` → `FaceData` (shared blob + face index), `face_info()`,
  `embedding_denied()` (fsType), `substitutions()`, `glyph_outline()`,
  `glyph_outline_normalized()`, `normalized_coords()`, `units_per_em()`.
  `FaceId` is a dense `u32` index, stable for the database's life.
- **`Shaper`** (`shape.rs`, `layout.rs`) — `Shaper::new(Arc<FontDb>)`,
  `layout(&StoryInput) -> Layout`. Implements **`FontMetrics`**
  (`char_metrics`, `kern_pair`), the `FormatRegion` replacement.
- Input types (`style.rs`): `StoryInput { text, runs: &[StyleRange],
  paragraphs: &[ParagraphStyle], kerns: &[ManualKern], mode: StoryMode }`,
  `FontQuery`, `FontFeature`, `FontVariation`, `TextScript`, `Justification`,
  `LineSpacing`, `TabStop`/`TabKind`, `Direction`.
- Output (`layout.rs`): `Layout { lines, bounds, substitutions }`,
  `LaidLine { logical_range, paragraph, ends_paragraph, baseline_y,
  descent_line, ascent, descent, size, x, width, runs: Vec<GlyphRun>,
  clusters: Vec<LaidCluster> }`, `GlyphRun { face, size, aspect, coords,
  style, level, glyphs }` with `glyph_transform(g, upem) -> kurbo::Affine`,
  `PlacedGlyph { id, x, y, advance, cluster }`.

## Decisions taken (and why)

- **Dependencies.** `parley 0.11.1` + `fontique 0.11.1` (Apache-2.0 OR MIT),
  `skrifa 0.44` (MIT OR Apache-2.0; the version parley 0.11.1 links, not the
  0.47 of `research/05` — one skrifa in the graph, shared with `vello`),
  `unicode-bidi 0.3.18` (MIT OR Apache-2.0, already in the graph),
  `icu_segmenter`/`icu_properties 2.3` (Unicode-3.0, already allowed).
  `harfrust 0.12` arrives through parley. **No `rustybuzz`/`ttf-parser`**
  (RUSTSEC-2026-0192/0206). No `deny.toml` exception was needed.
- **`fontconfig-dlopen`** on fontique, like winit's `wayland-dlopen`: the
  build needs no fontconfig headers and the AppImage uses the host's
  libfontconfig, which it must (a bundled one cannot see the user's fonts).
- **The parley font context lives inside `FontDb`'s lock**, and the shaper
  locks the database while it shapes. So shaping sees exactly the faces the
  database knows (embedded ones included) with no collection clones to keep
  in sync. Cost: one `FontDb` serialises shaping across threads. Parallel
  layout later wants one database per worker or a shared fontique
  collection (`CollectionOptions::shared`), not a bigger lock.
- **Shape at a nominal 1000 units per em, convert once per glyph,
  accumulate in millipoints** (phase doc W9.3). Every run is shaped at
  `NOMINAL_SIZE = 1000.0` regardless of its size; an `f32` advance `v`
  becomes `round_half_away_from_zero(v × em / 1000)` mp, where `em` is the
  run's size × aspect (vertical offsets use the size). Guarded by
  `positions_accumulate_in_millipoints_without_drift`: a 500-character
  line ends at exactly 500 × one rounded advance, at 10 pt and at 9.973 pt.
- **We do our own line breaking, justification and line placement, not
  parley's.** Parley breaks and aligns in `f32` with CSS rules. Xara's rules
  (below) differ: width excluding the last character's tracking, spaces
  hanging, full-justification slack split between spaces and letters. So
  parley is asked for one unbroken line per paragraph and we use its
  clusters. Break opportunities come from ICU4X `LineSegmenter` directly
  (dictionary mode with the default `complex-scripts` feature): parley's
  `Cluster::is_word_boundary()` conflates word and line boundaries.
- **Bidi levels from `unicode-bidi`, not parley.** Parley resolves bidi but
  does not expose levels (only `is_rtl()` per run), and per-line visual
  reordering after our own line breaking needs them (UAX #9 L1-L2,
  `ParagraphBidiInfo::visual_runs`). Parley always auto-detects the base
  direction, so an explicit `Direction::Ltr/Rtl` is forced into it by
  prefixing U+200E/U+200F to the shaped text (and subtracting its length
  from every offset). Both sides therefore agree on the levels.
- **A cluster is never split.** Parley reports a ligature or a base with its
  marks as one "ligature start" cluster holding the glyphs plus
  "continuation" clusters with no glyphs and an equal share of the advance.
  `clusters()` walks text order, so continuations **follow** their start in
  an LTR run and **precede** it in an RTL run (lam-alef: lam is the
  continuation). We fold each group into one `ShapedCluster`, summing the
  `f32` advances and converting once.
- **Grapheme clusters are capped at 64 characters.** Parley 0.11.1 counts a
  cluster's characters in a `u8` (`src/shape/mod.rs:270`); "a" + 2 000
  combining marks overflowed it (a panic with overflow checks, silent wrap
  in release). `guard_long_graphemes` overwrites every character past the
  64th of a grapheme with U+0001 filler of the same byte length (offsets
  stay valid) and the filler's clusters are folded into the grapheme's head
  with no glyphs. Real text never gets close (stream-safe text allows 30
  non-starters). Worth reporting upstream.
- **Manual kerns and tracking are thousandths of an em**, not millipoints —
  see the facts below. `StyleRange::tracking` is `i32` em/1000 and
  `ManualKern::amount` too; both are converted with the em width of the
  style in force (`em = effective size × aspect`, via `Mp::mul_ratio`,
  rounding half away from zero).
- **Full-justification remainder is distributed**, 1 mp at a time to the
  first recipients, so a justified line is exactly its column's width
  (acceptance criterion 6 then holds to 0 mp). The original truncates the
  per-gap share and drops the remainder; the difference is < 1 mp per gap.
- **The substitution ladder** (T9.1.5) as implemented in
  `font/substitute.rs`: (1) exact name, case- and whitespace-insensitive
  (fontique's own lookup is case-insensitive; we collapse whitespace);
  (2) trailing style words stripped (`thin … black`, `italic`, `oblique`…;
  **not** `roman`, which ends "Times New Roman"), their weight/italic applied
  to the query; (3) metric-compatible aliases; (4) the generic family, from
  PANOSE when the document has it (digit 1 = 2 Latin text; digit 4 = 9
  monospaced; digit 2 = 11–13 sans, 2–10/14–15 serif) else from the name;
  (5) `system-ui`, then `sans-serif`, then the first family in sorted order.
  Every substitution is recorded (`FontDb::substitutions`,
  `Layout::substitutions`) and never written back to the document.
- **The alias table's contents and provenance**: groups {Arial, Helvetica,
  Liberation Sans, Arimo, Nimbus Sans, Nimbus Sans L, TeX Gyre Heros,
  FreeSans}, {Arial Narrow, Liberation Sans Narrow, Nimbus Sans Narrow},
  {Times New Roman, Times, Liberation Serif, Tinos, Nimbus Roman, Nimbus Roman
  No9 L, TeX Gyre Termes, FreeSerif}, {Courier New, Courier, Liberation Mono,
  Cousine, Nimbus Mono PS, Nimbus Mono L, TeX Gyre Cursor, FreeMono},
  {Calibri, Carlito}, {Cambria, Caladea}, {Georgia, Gelasio}. Written by us
  from the public fact that these families were published as
  metric-compatible replacements; not transcribed from Xara or from any
  fontconfig configuration.
- **Embedded faces shadow system faces.** `register_embedded` registers into
  fontique's own ("ours") family map, which `family_id()` consults before
  the system map. So once a document registers "Noto Sans", a query for any
  Noto Sans weight gets the embedded face (synthesised bold if need be),
  never the installed family. Asserted against real system fonts in the
  opt-in test.
- **Embedding denied** when `OS/2.fsType & 0xF == 2` (restricted licence)
  or bit 9 (bitmap embedding only) is set; unreadable tables count as
  installable, the OpenType default.
- **Glyph outlines** are drawn unhinted at unit scale into `BezPath` and
  cached per `(face, glyph, coords)` with trailing zero coordinates
  stripped, so `[]` and `[0]` hit the same entry. The cache is cleared
  wholesale at 65 536 entries rather than evicted. Drawing at a size is
  `GlyphRun::glyph_transform` (scale `size/upem`, × aspect horizontally,
  then translate), never a re-extraction.
- **Coordinates are y up**, like the document: the first baseline is y = 0,
  later ones negative; x = 0 is the column's left edge (column mode) or the
  anchor (point mode). Glyph y offsets from parley (y down) are negated.

## The text tool (W9.4, as built, XARA-US-0047)

Code: `xarast-app/src/text_edit.rs` (`CaretMap`, pure, story space),
`xarast-app/src/text_tool.rs` (`TextTool`, the state machine), `tools.md`
decisions 53–56 and `ui.md` decision 40 for the tool and shell sides.

- **A caret is a byte offset of the laid-out text plus an affinity**
  (`Caret { byte, upstream }`), not a `TextPos`: layout, hit testing and
  bidi all speak byte offsets, and a `TextPos` cannot say which side of a
  soft line break or a direction boundary it means. `TextSelection::cursor`
  derives the model's `TextCursor` (line, item) through
  `StoryText::pos_of` when something needs it. The laid-out text is
  `layout_text` (the story's last `'\n'` stripped), so the last caret
  offset is before the final EOL item.
- **Stops.** A line's caret stops are its clusters' two edges in x order:
  an LTR cluster's left edge is its start (downstream) and its right edge
  its end (upstream); an RTL cluster the other way round. Neighbouring
  clusters share an x: the same offset in one-direction text, two
  different offsets at a direction boundary. A cluster is never split, so
  a ligature or a base with its marks is one stop (T9.3's rule).
- **Visual movement** (the default; `TextTool::logical_arrows` is the
  preference) walks stops by x: right = the first stop further right,
  which is the trailing edge of the cluster crossed; left = the nearest
  stop further left. Off a line's end it goes to the logical start of the
  next line or the logical end of the previous one, by the line's base
  direction (`LaidLine::base_rtl`, added for this). Logical movement,
  word movement (UAX #29 via `xarast_text::word_segments`, dictionary
  data with `complex-scripts`), Home/End (logical line start/end: the right
  end of an RTL line is its start), Up/Down with a goal x kept across the
  run, Page Up/Down (lines per view height) and Ctrl+Home/End are the
  other motions. Up on the first line goes to the story start, Down on the
  last to its end (the macOS behaviour; Windows stays put — pick again if
  the maintainer prefers). Arrow keys pointing right in an RTL paragraph
  mean logical *backward* for logical and word motion.
- **Split caret.** The primary caret is the stop the caret sits on (its
  offset *and* side), so repeated arrows move it steadily across the
  screen; the secondary, drawn half height, is the other place the same
  offset is drawn at a direction boundary. Not Pango's strong/weak rule
  (strong = paragraph direction): with that one the drawn caret jumped
  back across the Hebrew run while the user pressed Right (dead end).
- **Soft line ends.** `line_of` shows an offset shared by two lines on the
  upper one when upstream, the lower one otherwise. End on a wrapped line
  gives an upstream caret at the line's end (trailing spaces included);
  Right from there moves to the start of the next line *at the same
  offset* (visual-only move) and Left comes back.
- **Hit test**: nearest line by its band (descent below to ascent above
  the baseline), then the nearest stop; between two stops at one x the
  one on the pointer's side wins.
- **Selection spans**: per line, the selected clusters' boxes merged left
  to right (so mixed text gives several spans, in visual order), plus a
  block a third of the line's size wide at the line's logical end when the
  paragraph break after it is selected.
- **Pending caret.** A click on empty canvas or a column drag makes a
  *pending* caret, not a story; the first typed character creates it
  (below).
- **Text on a path** (`OnPath`): the caret follows the straight layout the
  walker draws until W9.5.

Measured (`cargo bench -p xarast-app --bench text_caret`, 10 000
characters, Latin + Hebrew, 300 pt column, pinned fonts; budget 1 ms per
motion): visual right 0.72 µs, logical right 2.1 µs (122 µs before the
move looked only at the caret's line and its neighbours), word right
1.1 µs, line down 1.2 µs, line end 0.40 µs, hit 0.55 µs, selection spans
of the whole story 54 µs.

Tests: `xarast-app/tests/text_caret.rs` (15: every assertion on byte
offsets — Latin, RTL, mixed walks, split caret, hit round trip, right n /
left n, selection spans, the paragraph-break block, soft line ends, goal
x, words, empty story, marks; plus the corpus walk over every line of
`hebrew.xar`: visual order strictly rightwards, every stop visited),
`tests/text_tool.rs` (10, through `Session` intents: document untouched),
`xarast-shell` `a_text_caret_takes_the_navigation_and_character_keys`,
`xarast-ui` `the_caret_blinks_from_on_then_stops_blinking`.

## Typing and deleting (T9.4.6, as built, XARA-T-0223)

Code: `xarast-doc/src/text_edit.rs` (`InsertText`, `DeleteRange`,
`insert_text`, `delete_range`, `new_story`), `xarast-app/src/ops.rs`
(`EditCommand::{TypeText, DeleteText, CreateText}`), `text_tool.rs`
(bursts), `xarast_text::{prev_grapheme, next_grapheme}`.

- **Offsets, not `TextPos`.** The edit commands take byte offsets into
  `StoryText::text` (the phase doc sketches `TextPos`): the tool, layout
  and hit testing speak offsets, and an offset survives the line split an
  edit makes where a `(line, item)` pair does not.
- **What an insertion does to the tree.** One `TextItem` node per
  character, right after the character before the offset, so new text
  takes that character's style; that character's own attribute children
  are copied onto each new item. After a paragraph break or at a line's
  start the items go before the first item at the offset instead.
  `'\r\n'`/`'\r'` → `'\n'`, `'\t'` → `Tab`, other controls dropped.
- **`'\n'` splits the line**: a `LineBreak(true)` is inserted and
  everything after it moves to a new `TextLine` right after, which starts
  with copies of the attributes the line had in scope at the split (last
  per slot) and the same ruler. No character changes style (asserted),
  and every line still ends with its break.
- **Deleting a paragraph break does not merge lines**: the line simply no
  longer ends with a break, and `paragraph_first_lines` runs the paragraph
  on into the next line (paragraph style from the first line). Merging
  would need explicit attributes to keep the moved characters' style;
  leaving the lines costs nothing (they are derived state; `format_story`,
  T9.3.12, will re-line). A line left with no items is removed unless it
  is the story's last. Kerns/soft breaks strictly inside a deleted range go
  with it; one at the range's start stays.
- **Undo record = the per-node actions**, proportional to the text typed
  or deleted. The phase doc's "store the paragraph's item run before and
  after" (and its 4 KiB diff threshold) is not needed while nothing
  reflows into the arena: there is no wrap restructuring to undo. Revisit
  with T9.3.12.
- **New story** (`new_story`): `TextStory` (matrix = translate to the
  pending caret, `AtPoint` or `InColumn { width, word_wrap: true }`) →
  the current attributes as its own children → one `TextLine` holding only
  `LineBreak(true)`, the final EOL every story ends with (so the caret's
  last offset is before it, as for imported stories). Then the text is
  inserted at 0.
- **Grapheme-aware deletion**: Backspace/Delete remove one extended
  grapheme cluster (ICU4X; a base with its marks, a ZWJ emoji sequence,
  CRLF), not one code point; Ctrl deletes to the logical word motion's
  target (Ctrl+Delete = up to the next word's start, as Ctrl+Right). A
  selection is deleted whole; typing replaces it. The final EOL is outside
  the laid-out text, so it can never be deleted from the tool.
- **Typing bursts** (`tools.md` decision 57): one undo step per burst,
  merged by `CoalesceKey { gesture: burst, kind: "text-typing" |
  "text-delete" }` in the history.

Tests: `xarast-doc` `text_edit::tests` (5: style inheritance, line split
keeps every style, joining paragraphs, bad offsets refused, new story),
`xarast-text` `graphemes_step_over_marks_and_emoji_sequences`,
`xarast-app/tests/text_tool.rs` (6 new: 200 characters = one undo step,
burst ends, graphemes, selection replace + Enter, pending → story in one
step, column wrap), `xarast-shell`
`a_text_caret_takes_the_navigation_and_character_keys` (typing via key
events).

## Walker integration (as built, round 2)

- **Model (`xarast-doc/src/text_model.rs`).** `StoryText::collect(tree,
  story, &mut AttrStack, attr_value)` walks one story with the render
  walk's attribute stack (so each character resolves exactly as it would
  paint) and returns `text`, `runs: Vec<CharRun { range, attrs:
  ResolvedAttrs }>` (a new run only when an attribute node was pushed or a
  scope popped), `lines: Vec<LineEntry>` (line-level attributes = the
  snapshot at the line's first item), `items` (the `TextPos` index),
  `kerns: Vec<KernAt { at, amount }>`. `byte_of(TextPos)` / `pos_of(byte)`
  are binary searches. `StoryFlow::of(story)` reduces `TextLayout`. An
  item's own attribute children apply to it (painted at the end of their
  scope). Nothing is written to the tree.
- **Bridge (`xarast-app/src/text.rs`).** `story_input` is the table above;
  the typeface's PANOSE now travels in `StyleRange::panose` so the ladder's
  PANOSE rung works during layout. The **last** `'\n'` of a story is not
  given to layout: every `.xar` line ends in an EOL, and a trailing break
  would open an empty paragraph the original never measures (it made
  `hebrew.xar`'s frame twice as tall).
- **Geometry.** `build_story` lays out, then per glyph applies
  `story.transform × glyph_transform` (plus a shear for faux italic when
  `FontMatch::synthesis.skew` says so) and appends the outline to **one
  `BezPath` per style run**, then converts to a millipoint `Path`. Glyph
  outlines fill **non-zero**, whatever the winding attribute. Underline is
  a rectangle a tenth of the size below the baseline, a twentieth thick
  (not the font's `post` values yet).
- **Walker.** A `TextStory` is painted whole at its `Visit` and its
  subtree skipped. Each run emits fill / stroke / transparency like any
  object, so gradients (document-space geometry) span the story as in the
  original. The `StoryGeometry` is cached per `NodeId` and dropped when
  `Document::epoch` moves; its content hash folds every node version in the
  story. `WalkStats` gains `text_stories`, `text_on_path_pending`;
  `text_pending` now means "visible text with no inked glyph" (no font at
  all). `SceneWalker::text_ink()` is the drawn text's box: `Session` adds
  it to `scene_ink`, and `viewport::drawing_rect_with` lays stories out for
  framing (text has no cached bounds).
- **Fonts (`xarast-app/src/fonts.rs`).** `FontService { FontDb, Shaper,
  OnceLock<usize> }`; `ready()` enumerates once (on the caller's thread if
  nobody started it). `fonts::shared()` is the process service: system
  fonts, or `XARAST_FONT_DIR` / `set_shared(FontService::from_dir(dir))`
  for pinned fonts (all generics → "Noto Sans", every family a fallback for
  Hebrew/Arabic/Han/kana). The shell calls `fonts::prewarm()` before the
  window. Tests pin fonts: `xarast-app/tests/{corpus,text,xarast_roundtrip}.rs`
  and `xarast-cli/tests/cli.rs` (env var).
- **Substitutions are never silent.** The walker records each once
  (`font_substitutions()`); `Session::take_font_substitutions` hands out the
  new ones; `AppState::collect_font_substitutions` puts them in the problem
  list as warnings and the viewer shows the newest in the status bar; the
  CLI prints `font substituted: A -> B` on stderr.
- **Importer (`xarast-xar`).** A string's/character's attribute children
  apply to **it alone** (`Kernel/cxftext.cpp:1260-1300`: a string is a run
  of characters with identical attribute children; `Kernel/impstr.cpp`
  copies them to every character). They were emitted after the characters,
  which handed each string's style to the next one (visible in
  `FontChangesInText.xar`). Now they go before, and what they overrode is
  restored before the next text record that does not set the slot; the
  mapper mirrors the emitted attribute state (`AttrStack`) to know that
  value. Non-BMP characters written as two `TAG_TEXT_CHAR` records pair.
  A story that relies on the original's default size gets an explicit 16 pt
  attribute (below).

### The acceptance fixture, round 2 (pinned: this machine's fonts)

Every `TextDesigns` file carries a **bitmap of the original's own
rendering** under the text (the "red/green ghost" in our renders), which
makes each file a visual oracle. With the host's metric-compatible fonts
(Arial → Liberation Sans) `SimpleText`, `Paragraph`, `TextJust`,
`LineSpacing`, `BaselineShift`, `SuperSub`, `Tracking`, `ManualKern` and
`Kerning` overlay their reference within about a pixel at 250 %.
`FontChangesInText`, `Rotated` and `AngledText` differ only where Calisto MT
/ Book Antiqua were substituted (Noto Serif is wider). `hebrew.xar`'s story
has no typeface (default Times New Roman); Windows' Times New Roman has
Hebrew, Liberation Serif does not, so the fallback face is larger and
serif. `embeddedFonts.xar` embeds **no** font data: its Calligraphic,
Margaret and Myriad Web are substituted.

Missing on this machine: Arial, Times New Roman, Courier New (metric
aliases used: Liberation Sans/Serif/Mono), Calisto MT, Book Antiqua (→
Noto Serif via PANOSE), Calligraphic, Margaret, Myriad Web (→ Noto Sans),
KirbysHand and Swis721 Blk BT (defined in files, never reached a laid-out
run).

## Facts about the original's formatter (read, not copied)

- **The em of tracking and manual kerns is the advance of `'M'`**, not the
  point size: `FontEmWidth` is the cached width of `FONTEMCHAR`
  (`wxOil/textfuns.h:114`, `wxOil/fontbase.cpp:945-956`), scaled by the
  aspect. About 0.83 em for Arial, 0.907 for Noto Sans. `FaceMetrics::
  em_char_advance` reads it; layout scales `StyleRange::em_width()` by it.
  Before this, `Tracking.xar` was ~20 % too wide; now it overlays.
- **The default font size is 16 pt** (`Kernel/txtattr.cpp:449-452`); the
  default typeface is "Times New Roman" (`Kernel/fontman.h:111`). The
  model's `default_for(TxtFontSize)` is 16 pt since XARA-T-0172 (the
  `.xarast` writer always writes a run's size and the reader's fallbacks
  use `default_for`); the importer's explicit 16 pt is no longer needed
  and no longer written.

Taken from the original to fix semantics; implemented from these notes.

- **Tracking unit** (settles `research/01 §11` item 15 and
  `xar-import.md` open question 1): `AttrTxtTracking` holds thousandths of
  an em. The advance is `CharWidth + MulDiv(tracking, FontEmWidth, 1000)
  (+ autokern)` (`Kernel/nodetext.cpp:1781-1792`); `FontEmWidth` is scaled by
  the X scale, i.e. it includes the aspect ratio (`wxOil/textfuns.cpp:151-157`).
- **Manual kerns** (`KernCode`) are thousandths of an em too:
  `MulDiv(KernValue.x, FontEmWidth, 1000)` (`Kernel/nodetext.cpp:1763-1767`).
  The importer stores `TextItem::Kern(Mp)` with that raw value — it is em/1000,
  whatever the type says.
- **Line width for alignment** is the sum of full advances up to the last
  non-space character, plus that character's *width* (advance without
  tracking or autokern); trailing spaces do not count
  (`Kernel/nodetxtl.cpp:1438-1491`).
- **Alignment** (`Kernel/nodetxtl.cpp:1530-1579`): left starts at the left
  margin; right at `right − width`; centre at `(left + right − width) / 2`
  (C integer division). Physical right = story width for a column, 0 for
  point text, so point text aligns about its anchor.
- **Full justification**: if the line wraps, is the last of its paragraph
  and is short, it is left aligned. Otherwise `gap = (right − left) −
  width`; if `gap > 0` and the line has spaces, every space gets
  `gap / spaces`; if `gap > 0` without spaces (and the story wraps), or
  `gap ≤ 0`, every character gets `gap / (chars − 1)` (letter spacing can
  shrink). Counts stop at the last non-space character and **restart after
  each tab**: only the last tab section stretches. "Characters" includes
  spaces.
- **First line of a paragraph uses the first-line indent *instead of* the
  left margin** (`Kernel/nodetxtl.cpp:780-795`), not in addition.
- **Line breaking** (`FindBreakChar`, `Kernel/nodetxtl.cpp:1262-1362`):
  spaces never overflow and are break points; a break is allowed after a
  hyphen and before a tab; a character fits if its width (no tracking) fits;
  at least one character per line; an overlong word breaks before the
  character that overflows. Our UAX #14 opportunities are a superset of the
  original's (they add CJK and the like).
- **Line spacing** (`CalcBaseAndDescentLine`, `Kernel/nodetxtl.cpp:1600-1640`):
  with `h = ascent + descent` of the line; **absolute** `S`: each line box
  is `S` tall and the baseline sits `S × descent / h` above the box bottom;
  **ratio** `r`: the box is `h × r` tall and the baseline sits
  `ascent × min(r, 1)` below the previous box's bottom. The first baseline
  is 0. `LineSpacing::Absolute(0)` means "use the ratio".
- **Default tab stops** every 36 000 mp (half an inch), strictly after the
  current position (`Kernel/txtattr.cpp:3181-3205`).
- **Ruler record** (`Kernel/rechtext.cpp:1546-1575`): per stop,
  `type = flags & 3` (0 left, 1 right, 2 centre, 3 decimal), bit 2 "has a
  filler character"; a decimal stop carries its decimal-point character.
- Line ascent/descent/size are the maxima over the line's characters; an
  empty line takes them from its end-of-line item.

## The contract for W9.2 (`xarast-doc`, next round)

What the document layer builds, and how `format_story` feeds `Shaper`. The
existing model (`crates/xarast-doc/src/text.rs`, `attr/`) already has most
of the structure; this is what to add and how to bridge.

### Nodes (existing, keep)

- `NodeKind::TextStory(Box<TextStoryNode { transform, layout: TextLayout,
  auto_kern, print_as_shapes }>)`, `NodeKind::TextLine(Box<TextLineNode {
  ruler }>)`, `NodeKind::TextItem(TextItem::{Char(char), Kern(Mp),
  Tab, LineBreak(bool)})`. Invariant 11: items only under lines, lines only
  under stories.
- Lines are **formatted lines**; `LineBreak(true)` is the paragraph end,
  word wrap produces lines without one. Lines are derived state: undo
  restores the item list and reflows (phase doc W9.2). Proposed diff
  threshold: 4 KiB of items per paragraph, above which the inverse stores a
  diff; not implemented yet.

### Add to the model

- `TextPos { line: NodeId, item: u32 }` and the story's **logical text**:
  the concatenation of the items in document order where `Char(c)` → `c`,
  `Tab` → `'\t'`, `LineBreak(true)` → `'\n'`, `LineBreak(false)` → nothing
  (it is not produced by the importer and should not survive reflow), and
  `Kern` → nothing (it becomes a `ManualKern` at the byte offset of the
  next character). Conversion `TextPos ↔ byte offset` walks this mapping;
  keep a per-story `Vec<(NodeId, u32 /*first byte*/)>` index, rebuilt on
  format, so it is O(log n).
- `TextCursor { story, anchor: TextPos, head: TextPos }` in the tool's state
  (not in the tree).
- Derived per-line cache, outside the arena: the `LaidLine` of each
  `TextLine` (baseline, ascent/descent, glyph runs) keyed by line node, plus
  the story's `Layout`. The arena keeps no metrics.

### Attribute bridge (`AttrValue` → `StyleRange` / `ParagraphStyle`)

Resolve the attribute stack once per run of identical character attributes
(Xara's `FormatRegion` role) and emit one `StyleRange` per run, byte ranges
in the logical text:

| Attribute | Goes to | Conversion |
|---|---|---|
| `FontTypeface(Arc<TypefaceRef { family, full_name, panose }>)` | `FontQuery.family` | `family` (not `full_name`); pass `panose` to `FontDb::query_with_panose` |
| `Bold(b)` / `Italic(i)` | `FontQuery.weight` / `.style` | 700/400, `Italic`/`Normal`; stretch 100 |
| `FontSize(Mp)` | `StyleRange.size` | as is |
| `Tracking(Mp)` | `StyleRange.tracking: i32` | **raw value, it is em/1000** (the `Mp` type is historical) |
| `AspectRatio(f32)` | `.aspect` | as is |
| `Baseline(Mp)` | `.baseline_shift` | as is, positive up |
| `Script(Script { on, offset, size })` | `.script: TextScript` | `on == false` → `TextScript::NONE`, else `{ offset, size }` |
| `Underline(b)` | `.underline` | as is |
| `Justification` | `ParagraphStyle.justification` | same four values |
| `LineSpace(LineSpacing::{Ratio, Absolute})` | `.line_spacing` | same shape |
| `LeftMargin` / `RightMargin` / `FirstIndent` | `.left_margin` / `.right_margin` / `.first_indent` | as is |
| `Ruler(Arc<[TabStop { position, kind: u8 }]>)` | `.tabs` | `kind & 3`: 0 Left, 1 Right, 2 Centre, 3 Decimal. The decimal-point character is not in the model yet: add it before T9.3.8 |
| `TextStoryNode.auto_kern` | every `ParagraphStyle.auto_kern` | story-wide |
| `TextItem::Kern(Mp)` | `ManualKern { at, amount }` | `amount` = raw value (em/1000), `at` = byte offset of the following character |

Line-level attributes live under `TextLine` nodes. A paragraph takes its
`ParagraphStyle` from its **first** line (the lines of one paragraph carry
the same line-level attributes); `StoryInput::paragraphs` has one entry per
`'\n'`-separated paragraph.

### Story mode bridge

| `TextLayout` | `StoryMode` |
|---|---|
| `AtPoint` | `Point` |
| `InColumn { width, word_wrap }` | `Column { width, wrap: word_wrap }` (a non-wrapping column still aligns to its width) |
| `OnPath { .. }` | W9.5: lay out as `Point`, then fit to the path (spike A) |

`TextStoryNode.transform` (the `.xar` story matrix) is applied by the
renderer on top of the story-space layout; `Layout` never includes it.

### What the `.xar` importer already keeps (W3.10, "structure only")

From `crates/xarast-xar/src/import.rs`: stories (2100/2101/2110–2117 with
the matrix and auto-kern), `TAG_TEXT_STORY_WORD_WRAP_INFO` → `InColumn`,
indents, lines (2200), strings (2201) and chars (2202) → `Char`, EOL (2203)
→ `LineBreak(true)`, tab → `Tab`, kern → `Kern(dx)`, and the 2900–2920 and
4201–4204 attributes above. Font definitions (2000/2001) become
`TypefaceRef` with PANOSE. `TAG_TEXT_LINE_INFO` (2206, the original's cached
line metrics) is skipped — our layout recomputes it; it could serve as a
cross-check oracle in W9.7. Two importer issues for W9.2.6:

1. `TAG_TEXT_CHAR` is one UTF-16 code unit and is mapped with
   `char::from_u32(u16)`, so a non-BMP character stored as two `TEXT_CHAR`
   records becomes two U+FFFD. Pair surrogates across consecutive records.
2. The `Tracking` and `Kern` values are em/1000 (above); the conversion
   happens in `format_story`, so the import can stay raw, but the doc
   comments that call the unit "unsettled" should be updated.

### `format_story` sketch

```text
format_story(tree, story, shaper):
    text, index, runs, paras, kerns = collect(story)   // attribute resolution
    layout = shaper.layout(&StoryInput { text, runs, paras, kerns, mode })
    restructure lines to match layout.lines (wrap = move items between
        TextLine nodes; never touch LineBreak(true))
    cache layout per line; return substitutions for the UI
```

## Invariants that must not be broken

1. **Nothing outside `xarast-text` names a parley, fontique or skrifa type.**
   The public API uses our own types (`FaceData` wraps the blob).
2. **Positions are accumulated in millipoints**, never in `f32` across a
   line. Only per-glyph values come from `f32`.
3. **Tests never see system fonts** unless `XARAST_SYSTEM_FONT_TESTS=1`.
   Deterministic tests use `FontDb::new_isolated()` and the pinned set in
   `crates/xarast-text/tests/fonts/` (provenance and SHA-256 in
   `PROVENANCE.md`). Golden renders in W9.7 must do the same: a render that
   depends on the CI machine's fonts is not a test.
4. **Byte offsets are story-global** in every input and output (`StyleRange`,
   `ManualKern`, `PlacedGlyph::cluster`, `LaidCluster::range`,
   `LaidLine::logical_range`), and every cluster boundary is a char
   boundary.
5. **A grapheme longer than 64 characters is truncated for shaping, never
   passed to parley whole** (see above).
6. **`LaidCluster`s are in logical order; glyph runs in visual order**, left
   to right.

## Test fonts (pinned set)

`crates/xarast-text/tests/fonts/`, ~200 KB: Noto Sans Regular/Bold/Italic
subsets, Noto Sans Hebrew, Noto Sans Arabic, Noto Sans CJK JP (CFF), all
SIL OFL 1.1 from Debian `fonts-noto-core` 20201225-2 and `fonts-noto-cjk`
1:20230817+repack1-3, no Reserved Font Name (checked in name IDs 0/13/14
and the Debian copyright files), licence text in `OFL.txt`; plus
`XarastTestVariable.ttf`, a synthetic `wght` font we drew
(`make_variable.py`, MIT OR Apache-2.0). `make_subsets.sh` regenerates the
subsets (fontTools 4.65.0). Golden glyph ids in `tests/shaping.rs` are
pinned to these bytes.

## Measurements

Reference machine (`perf.md`), `cargo bench -p xarast-text --bench layout`,
pinned fonts, load ≈ 8:

| Budget | Target | Measured |
|---|---|---|
| Shape + lay out ~1 000 glyphs, justified column | ≤ 8 ms | **0.22 ms** |
| Shape + lay out 10 000 characters (20 paragraphs, 130 lines, Latin + Hebrew, alternating weights) | ≤ 60 ms | **2.28 ms** |
| Glyph outline, cached | ≤ 500 ns | **24 ns** |
| System font enumeration (fontconfig, 2 202 families) | ≤ 300 ms | **40 ms** (opt-in test) |

Binary size (release, `lto = "thin"`, stripped): the text stack costs
**≈ 1.8 MB** without the ICU4X dictionaries and **≈ 5.6 MB** with
`complex-scripts` (the Thai/Lao/Khmer/Myanmar dictionaries are ≈ 3.8 MB of
it), measured on a probe binary that enumerates, lays out and extracts
outlines. `xarast` itself declares `xarast-text` (through `xarast-app`) but
uses none of it yet: +1.2 KB today. The cost arrives when the app first
calls `Shaper`.

**Binary size, shipped (round 2, 2026-09-23).** Stripped release `xarast`
(`lto = "thin"`), before the walker called `Shaper` (1fa7fef) → after:
**21 981 784 → 27 689 320 bytes (+5 707 536, +5.44 MiB, +26 %)** with the
default `complex-scripts`; **23 897 192 (+1 915 408, +1.83 MiB)** without
it. The ICU4X Thai/Lao/Khmer/Myanmar dictionaries are therefore
**3 792 128 bytes (3.62 MiB, 14 % of the binary)**. Recommendation:
**keep** `complex-scripts` — without it a Thai, Lao, Khmer or Burmese
column (no spaces between words) can only break by emergency, which is a
correctness loss in a text tool; if the AppImage size budget bites, load
the dictionaries as data at run time (ICU4X `DataProvider`) rather than
drop them. `ldd` still shows no libwayland, GL, EGL, Vulkan or
fontconfig (fontique dlopens it).

**Start-up (window, `cold_start_ms` to first presented frame, 3 runs,
release, this machine, GPU tiles).** Text-free `Designs/BLUECAR.xar`:
368–427 ms before, 349–421 ms after (noise). Text-heavy
`TextDesigns/Rotated.xar`: 383–390 → 393–403 ms (+≈10 ms: layout and
outlines of 7 stories; enumeration runs in the background from
`fonts::prewarm`). `Designs/GardenPlan.xar` (text + 1 700 objects):
373–403 → 384–399 ms. The 400 ms cold-start budget (XARA-T-0010) is
unchanged in its status: at the edge with or without text. Headless, the
first story of a process waits for enumeration when nothing prewarmed
(CLI: `AngledText.xar` first walk 48 ms, of which ≈ 40 ms fontconfig).

## Dead ends (do not retry)

- ~~Changing the model's default font size to 16 pt~~ — done with
  XARA-T-0172 once the `.xarast` writer/reader stopped using 12 000 mp
  fallbacks of their own (they had broken the normal-form round trip on
  GardenPlan).
- **A `HeadlessOptions::fonts` field**: the struct is `Copy + PartialEq`;
  use `headless::render_with_fonts`.
- **Emitting a text record's attributes after its characters** (the
  structural importer's choice): the model scopes attributes to the
  *following* siblings.

- **Parley's line breaking and `Alignment::Justify`** for Xara-compatible
  layout: CSS semantics, `f32` positions, justification on spaces only.
- **`Cluster::is_word_boundary()` as a line-break opportunity**: it is true
  for word boundaries too.
- **Relying on `Cluster::is_ligature_start` order**: in RTL runs the start
  comes *after* its continuations in `clusters()`.
- **A `fontique` shared collection cloned into the shaper**: `System` fonts
  loaded after the clone are invisible to it (the system handle is per
  clone). Keeping the font context inside the database is simpler.
- **"Roman" as a style suffix**: it strips "Times New Roman" to "Times New".

## Open TODOs

- Round 2 leftovers: faux **bold** is not synthesised (skew is); underline
  uses fixed proportions, not the font's `post` table; `TabStop` decimal
  character still not in the model; the layer bounds cache (when warm)
  ignores text, so `drawing_rect` misses text once `update_bounds` has run;
  `Viewport::fit_bounds_to` (scroll bounds) ignores text; a per-document
  embedded-font overlay; W9.5 text on a path; golden images of
  `TextDesigns` with pinned fonts (W9.7).
- Typing leftovers (T9.4.6): a story emptied by deleting all its text stays
  (the original deletes an empty story when the caret leaves it; doing so
  here would add an undo step — decide with the maintainer); typed text
  does not pick up attributes chosen while the caret is up (needs
  `SetTextAttr`, T9.2.4); Unicode line/paragraph separators (U+2028/9)
  type as characters, not breaks.

## The `.xarast` text writer (XARA-T-0172, done)

- The profile carries a story exactly (`research/06 §6.7.1`,
  `docs/memory/xarast-format.md` "Text"): runs of items with every text
  attribute resolved and twinned, kerns / breaks / non-XML characters as
  elements in place. A reload resolves every character as before, so the
  walker lays it out and draws it identically: 59/59 corpus files render
  pixel-identical after a round trip with the pinned fonts.
- **For browsers** the writer asks a `TextPlacer`; `xarast-app`'s
  `svg_text::SvgTextPlacer` runs the same bridge and shaper as the walker
  (`text::story_input`, `layout_text`) and reports each character item's
  cluster box left edge on its baseline (`PlacedGlyph::y` of the
  cluster's first glyph; the line baseline when it has none). The app's
  save path (not written yet) should pass `svg_text::placer()` in
  `SvgOptions::text`, as `xarast-cli convert` does.
- An empty `TextLine` resolves its line attributes from the state *after*
  its scope closes (`StoryText::end_line`), so its own attributes never
  matter; the format writes and reads that outer state at story level.

- W9.1: T9.1.6 background enumeration (the API is ready; the app must call
  `load_system_fonts` on its I/O thread), T9.1.7 gallery, T9.1.8 Windows and
  macOS smoke tests. A per-document embedded-font overlay: today an embedded
  face registered in a shared `FontDb` is visible to every document.
- W9.3: T9.3.8 centre/right/decimal tabs (only left stops now), T9.3.10
  features/variations are plumbed per run but untested beyond `kern`/`liga`/
  `wght`, T9.3.11 layout cache (the text tool keeps its own per-epoch
  cache of `CaretMap`s; the walker another).
- W9.4 leftovers: typing and grapheme-aware deletion with undo per burst
  (T9.4.6, creates the pending story), IME preedit and the IME caret area
  from the text caret (T9.4.7), clipboard (T9.4.8), text infobar and
  OpenType panel (T9.4.9), the interactive ruler (T9.4.10); Ctrl+Up/Down
  by paragraph (they move by line now); the pending caret's height uses
  0.8/0.2 of the current size, not the face's metrics.
- Shaping across a soft line break is not redone: an Arabic word split by
  an emergency break keeps its joined forms. Reshape the two halves if it
  ever matters (only emergency breaks can split a word).
- Which vertical metrics the original used for `FontAscent`/`FontDescent`
  (hhea, typo or win); we use skrifa's (typo when `USE_TYPO_METRICS`, else
  hhea). Check against `TAG_TEXT_LINE_INFO` in the corpus.
- Report the parley `u8` cluster-length overflow upstream.
- `FontMetrics::kern_pair` shapes the pair twice; fine for measuring, not
  for a hot loop.
