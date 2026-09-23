//! The mapping stage: a record tree becomes a [`xarast_doc::Document`].
//!
//! Everything below this module is model-independent. This is where the
//! typed records of [`Decoded`] turn into nodes, attributes and resources,
//! and it is the only part of the crate that knows `xarast-doc` exists.
//!
//! # The importer never touches the arena
//!
//! Every node, attribute and resource goes through [`DocumentBuilder`].
//! `xarast-doc` makes that structural rather than a convention —
//! `Tree::attach` and its siblings are `pub(crate)` — and it is what
//! guarantees an importer cannot produce an inconsistent document:
//! `DocumentBuilder::finish` either fails or returns a document whose
//! `validate()` has no errors.
//!
//! # Attributes are children, so scope is the tree
//!
//! `.xar` writes an object's attributes inside the object's own `DOWN`/`UP`
//! (`research/01 §6.1`). The model has exactly the same rule — an attribute
//! applies to its following siblings *and to its parent's own ink* — so
//! pushing a builder scope on descent and popping it on ascent is the whole
//! of the inheritance mapping. There is no attribute stack to maintain.
//!
//! # What look-ahead is for
//!
//! Three records describe the node that *encloses* them: `TAG_PATH_FLAGS`
//! (the path's first child), `TAG_SPREADINFORMATION` and `TAG_LAYERDETAILS`.
//! A `DocumentBuilder` is append-only by design, so rather than mutate a node
//! after the fact the mapper reads those children before emitting the parent.
//! The record tree is already fully materialised, which makes that free; it is
//! also what the original does, under the name `ReadPostChildren`.
//!
//! # The spread coordinate origin
//!
//! See [`spread_origin`]. It is **not** `(0, 0)` for the great majority of
//! real files, and the corpus says so out loud.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xarast_color::{Colour, ColourValue, Transparency};
use xarast_doc::{
    ArrowSpec, AttrValue, BitmapData, BitmapId, BitmapInfo, BitmapResource, BuildLimits,
    DocumentBuilder, DocumentMeta, GridKind, GridNode, ImageFormat, LayerNode, NodeKind,
    OpaqueNode, OriginalEncoded, PageNode, Paint, PathNode, ProceduralParams, QuickShape, Ramp,
    RampStop, SpreadNode, TextItem, TextLayout, TextStoryNode, Tiling, TranspPaint, TypefaceRef,
};
use xarast_geom::{BiasGain, Cap, DashPattern, FillRule, Join, Matrix, Mp, Point, Rect, Vector};

use crate::colour::ColourRegistry;
use crate::decode::{
    Decoded, FillEffect, FillKind, FractalParams, GradientFill, GradientTransparency, LayerFlags,
    RegularShape, SpreadInformation, TextAttr, TextPlacement, decode,
};
use crate::diag::{DiagCode, DiagSink, Diagnostic, Severity};
use crate::error::XarError;
use crate::header::FileHeader;
use crate::paths::apply_path_flags;
use crate::reader::{BlockReport, ReaderLimits};
use crate::tags::{Ref, TagClass, class_of};
use crate::tree::{FileAnalysis, RecordNode, analyse};

/// How many [`NodeKind`] discriminants there are.
///
/// The counts in [`ImportReport::node_counts`] are indexed by
/// `NodeKind::discriminant()`, so this has to track the model. A test asserts
/// it does.
pub const NODE_KIND_COUNT: usize = 19;

/// The name of each [`NodeKind`] discriminant, in discriminant order.
///
/// These are facts about the model, never about a file, so a report built
/// from them is safe to commit.
pub const NODE_KIND_NAMES: [&str; NODE_KIND_COUNT] = [
    "Document",
    "Chapter",
    "Spread",
    "Page",
    "Layer",
    "Grid",
    "Path",
    "Shape",
    "QuickShape",
    "Bitmap",
    "Guideline",
    "Group",
    "Live",
    "ClipView",
    "TextStory",
    "TextLine",
    "TextItem",
    "Attr",
    "Opaque",
];

/// What an import is allowed to cost, and what it may leave out.
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// Caps on the physical layer.
    pub reader: ReaderLimits,
    /// Caps on the document being built.
    pub build: BuildLimits,
    /// Stop at the first `Error` diagnostic instead of carrying on.
    pub strict: bool,
    /// Drop text records entirely. Useful for bisecting a file that will not
    /// open.
    pub skip_text: bool,
    /// Keep bitmap metadata but not the embedded bytes.
    pub skip_bitmaps: bool,
}

/// What one import did. Facts only: no coordinate, colour or string taken
/// out of the file ever reaches this type, for the reason
/// [`crate::diag`] explains.
#[derive(Clone, Debug)]
pub struct ImportReport {
    /// The file header.
    pub header: FileHeader,
    /// Records the physical layer handed out.
    pub records_read: u32,
    /// Records that produced a node, an attribute, a resource or a document
    /// setting — or that were consumed by the node they describe, or that
    /// were `TAG_DOWN`/`TAG_UP` and became a builder scope.
    ///
    /// `records_mapped + records_skipped + records_stripped` equals
    /// [`ImportReport::records_read`]; [`ImportReport::records_opaque`] is a
    /// subset of `records_mapped`.
    pub records_mapped: u32,
    /// Records deliberately dropped: view state, printing, editor hints.
    pub records_skipped: u32,
    /// Records dropped together with an atomic subtree, by the tree builder.
    pub records_stripped: u32,
    /// Records with no decoder that were kept verbatim as
    /// [`NodeKind::Opaque`] so that they round-trip.
    pub records_opaque: u32,
    /// Occurrences per tag.
    pub distinct_tags: BTreeMap<u32, u32>,
    /// One entry per compressed block.
    pub blocks: Vec<BlockReport>,
    /// The deepest record nesting.
    pub max_depth: usize,
    /// Records that became a node of the *record* tree, which is what
    /// `records_mapped` and `records_skipped` between them account for.
    pub tree_nodes: usize,
    /// Nodes in the finished document, including the root.
    pub nodes_built: usize,
    /// Nodes per [`NodeKind`] discriminant; see [`NODE_KIND_NAMES`].
    pub node_counts: [u32; NODE_KIND_COUNT],
    /// Palette entries registered.
    pub colours: usize,
    /// Bitmap resources registered.
    pub bitmaps: usize,
    /// The coordinate origin the last `TAG_SPREADINFORMATION` implied.
    pub spread_origin: Point,
    /// Attributes read from `TAG_CURRENTATTRIBUTES` blocks. They are the
    /// editor's current attributes, not document defaults, and are skipped.
    pub current_attributes: u32,
    /// How many of those differed from
    /// [`DefaultAttrs::xara_compatible`](xarast_doc::DefaultAttrs::xara_compatible).
    pub current_differing: u32,
    /// Everything the `.xar` layers found.
    pub diagnostics: Vec<Diagnostic>,
    /// Everything the builder found, in the model's shared vocabulary.
    pub build_diagnostics: Vec<xarast_doc::Diagnostic>,
    /// How many `validate()` errors the finished document has. Always zero:
    /// `DocumentBuilder::finish` fails rather than return a document with
    /// any.
    pub validation_errors: usize,
    /// How many `validate()` warnings it has. `AttrAfterInk` is expected and
    /// common.
    pub validation_warnings: usize,
    /// Wall-clock time for the whole import.
    pub duration: Duration,
}

impl ImportReport {
    /// How many error-severity `.xar` diagnostics were raised.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// How many warning-severity `.xar` diagnostics were raised.
    #[must_use]
    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    /// Tags with no decoder whose [`TagClass`] is `Structural`.
    ///
    /// The number the phase's acceptance criteria count: an unhandled
    /// attribute loses a colour, an unhandled structural tag means the tree
    /// is not the tree the file describes.
    #[must_use]
    pub fn unknown_structural_tags(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .distinct_tags
            .keys()
            .copied()
            .filter(|&t| {
                !crate::decode::has_decoder(t) && class_of(t) == Some(TagClass::Structural)
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// The count for one [`NodeKind`] discriminant.
    #[must_use]
    pub fn nodes_of(&self, discriminant: u8) -> u32 {
        self.node_counts
            .get(usize::from(discriminant))
            .copied()
            .unwrap_or(0)
    }
}

/// Reads a `.xar` file and builds a [`Document`](xarast_doc::Document).
///
/// # Errors
///
/// [`XarError`] when the byte stream cannot be walked, and
/// [`XarError::Build`] when the document could not be built within
/// [`ImportOptions::build`].
pub fn import(
    bytes: &[u8],
    opts: &ImportOptions,
) -> Result<(xarast_doc::Document, ImportReport), XarError> {
    let start = Instant::now();
    let analysis = analyse(bytes, opts.reader)?;
    map(&analysis, opts, start)
}

/// Builds a document from an analysis that has already been done.
///
/// `xar-dump` uses this so that one read of a file serves both the facts
/// report and the model dump.
///
/// # Errors
///
/// As [`import`].
pub fn map(
    analysis: &FileAnalysis,
    opts: &ImportOptions,
    started: Instant,
) -> Result<(xarast_doc::Document, ImportReport), XarError> {
    let mut m = Mapper::new(opts);
    m.diags.absorb(&analysis.diagnostics);
    m.builder.meta(DocumentMeta {
        producer: analysis.header.producer.clone(),
        producer_version: analysis.header.producer_version.clone(),
        producer_build: analysis.header.producer_build.clone(),
        ..DocumentMeta::default()
    });
    m.visit_children(&analysis.tree.roots)?;

    // `TAG_DOWN` and `TAG_UP` never reach the tree as nodes — the tree
    // builder turned them into nesting — but they are mapped all the same,
    // onto `push_scope`/`pop_scope`. What is left once the tree's own nodes
    // and the stripped subtrees are taken out is exactly those, so adding
    // them here makes `mapped + skipped + stripped == records_read` hold
    // *and* keeps it a real check: it can only balance if every tree node
    // was visited exactly once.
    let structural = analysis
        .records_read
        .saturating_sub(analysis.tree.stripped)
        .saturating_sub(u32::try_from(analysis.tree.nodes).unwrap_or(u32::MAX));
    m.mapped = m.mapped.saturating_add(structural);

    let colours = m.colours.len();
    let bitmaps = m.bitmap_by_record.len();
    let origin = m.origin;
    let mapped = m.mapped;
    let skipped = m.skipped;
    let opaque = m.opaque;
    let current_attributes = m.current_attributes;
    let current_differing = m.current_differing;
    let diagnostics = m.diags.items().to_vec();

    // The builder validated the document as its last step; reuse that
    // report rather than walk half a million nodes a second time.
    let (doc, build_diagnostics, validation) = m.builder.finish_with_report()?;

    let mut node_counts = [0u32; NODE_KIND_COUNT];
    let mut nodes_built = 0usize;
    let mut count = |k: &NodeKind| {
        nodes_built = nodes_built.saturating_add(1);
        if let Some(slot) = node_counts.get_mut(usize::from(k.discriminant())) {
            *slot = slot.saturating_add(1);
        }
    };
    if validation.reachable == doc.tree.node_count() {
        // Every node alive is reachable, so the arena, in memory order, is
        // the same set as a walk from the root and much cheaper to visit.
        for (_, data) in doc.tree.iter() {
            count(&data.kind);
        }
    } else {
        for id in doc.tree.preorder(doc.tree.root()) {
            if let Some(k) = doc.tree.kind(id) {
                count(k);
            }
        }
    }

    Ok((
        doc,
        ImportReport {
            header: analysis.header.clone(),
            records_read: analysis.records_read,
            records_mapped: mapped,
            records_skipped: skipped,
            records_stripped: analysis.tree.stripped,
            records_opaque: opaque,
            distinct_tags: analysis.tree.histogram.clone(),
            blocks: analysis.blocks.clone(),
            max_depth: analysis.tree.max_depth,
            tree_nodes: analysis.tree.nodes,
            nodes_built,
            node_counts,
            colours,
            bitmaps,
            spread_origin: origin,
            current_attributes,
            current_differing,
            diagnostics,
            build_diagnostics,
            validation_errors: validation.errors.len(),
            validation_warnings: validation.warnings.len(),
            duration: started.elapsed(),
        },
    ))
}

/// The coordinate origin every point in a spread is written relative to.
///
/// `research/01 §5.3` states the rule — *the `lo` corner of the rectangle
/// enclosing every page of the spread* — but not how to compute that corner
/// from the four numbers `TAG_SPREADINFORMATION` actually carries. It is:
///
/// ```text
/// page_margin = if margin < bleed { margin + bleed } else { margin }
/// origin      = (page_margin, page_margin)
/// ```
///
/// A spread lays its first page out with its `lo` corner at
/// `(page_margin, page_margin)` in spread coordinates, and a double page
/// spread puts the second page one page-width to the right of it, so the
/// union's `lo` corner is the first page's either way
/// (`Kernel/spread.cpp:2196-2290`, `Kernel/rechdoc.cpp:640-644`).
///
/// **The corpus distinguishes this from `(0, 0)` decisively.** Every empty
/// template writes `TAG_VIEWPORT` as the degenerate rectangle
/// `(-margin, -margin, -margin, -margin)` and `TAG_CURRENTATTRIBUTEBOUNDS` as
/// `(-margin, -margin)`: both are an empty `DocRect` at spread-coordinate
/// `(0, 0)` with the origin already subtracted by the writer. `margin` is
/// 566 931 or 576 000 in 57 of the 59 files and 0 in `animation.xar`, whose
/// viewport is correspondingly `(0, 0, 0, 0)`. The origin is the margin.
///
/// What the corpus cannot distinguish is `margin` from
/// `margin < bleed ? margin + bleed : margin`, because every corpus file has
/// `bleed == 0`. `crate::synth` builds a file that does, and the unit tests
/// use it.
#[must_use]
pub fn spread_origin(info: &SpreadInformation) -> Point {
    let m = page_margin(info);
    Point::new(m, m)
}

/// The pasteboard margin actually used to place the pages.
fn page_margin(info: &SpreadInformation) -> Mp {
    if info.margin.raw() < info.bleed.raw() {
        info.margin.saturating_add(info.bleed)
    } else {
        info.margin
    }
}

/// The rectangle enclosing every page of the spread, in document
/// coordinates.
#[must_use]
pub fn pages_rect(info: &SpreadInformation) -> Rect {
    let lo = spread_origin(info);
    let pages = if info.double_page_spread() { 2 } else { 1 };
    let width = info.width.mul_ratio(pages, 1);
    Rect::new(
        lo,
        Point::new(lo.x.saturating_add(width), lo.y.saturating_add(info.height)),
    )
}

/// Where a record's children go.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum After {
    /// Into the node just emitted.
    Child,
    /// Onto the same level: the record itself produced no node.
    Same,
    /// Into the document's default attribute block.
    Defaults,
    /// Nowhere: the record handled them itself, or they are not wanted.
    Drop,
}

struct Mapper<'o> {
    builder: DocumentBuilder,
    colours: ColourRegistry,
    bitmap_by_record: HashMap<u32, BitmapId>,
    font_by_record: HashMap<u32, Arc<TypefaceRef>>,
    origin: Point,
    diags: DiagSink,
    opts: &'o ImportOptions,
    mapped: u32,
    skipped: u32,
    opaque: u32,
    current_attributes: u32,
    current_differing: u32,
}

impl<'o> Mapper<'o> {
    fn new(opts: &'o ImportOptions) -> Mapper<'o> {
        Mapper {
            builder: DocumentBuilder::new(opts.build.clone()),
            colours: ColourRegistry::new(),
            bitmap_by_record: HashMap::new(),
            font_by_record: HashMap::new(),
            origin: Point::ORIGIN,
            diags: DiagSink::new(),
            opts,
            mapped: 0,
            skipped: 0,
            opaque: 0,
            current_attributes: 0,
            current_differing: 0,
        }
    }

    // ── Traversal ───────────────────────────────────────────────────────────

    fn visit_children(&mut self, children: &[RecordNode]) -> Result<(), XarError> {
        // `TAG_GRIDRULERSETTINGS` (46) and `TAG_GRIDRULERORIGIN` (47) are two
        // sibling records describing one `Grid` node, so the first of them
        // emits the node and the other is consumed.
        let grid_at = children
            .iter()
            .position(|c| matches!(c.record.tag, 46 | 47));
        for (i, child) in children.iter().enumerate() {
            if matches!(child.record.tag, 46 | 47) {
                if grid_at == Some(i) {
                    self.emit_grid(children)?;
                }
                self.mapped = self.mapped.saturating_add(1);
                // A grid record has no children in any real file; anything
                // a corrupt one nests under it is not visited, so it is
                // accounted for here as skipped.
                self.skipped = self.skipped.saturating_add(count_subtree(&child.children));
                continue;
            }
            self.visit(child)?;
        }
        Ok(())
    }

    fn visit(&mut self, node: &RecordNode) -> Result<(), XarError> {
        let rec = &node.record;
        let at = (rec.number, rec.tag);
        let mut decoded = match decode(rec.tag, &rec.data, self.origin, &mut self.diags, at) {
            Ok(d) => d,
            Err(_) => {
                self.diags.push(
                    Diagnostic::new(DiagCode::TruncatedRecord)
                        .at(rec.number, rec.tag)
                        .with_detail(rec.data.len() as u64),
                );
                self.skipped = self.skipped.saturating_add(1);
                return self.visit_children(&node.children);
            }
        };
        if self.opts.strict && self.diags.count(Severity::Error) > 0 {
            return Err(XarError::Limit("strict: an error diagnostic was raised"));
        }
        match self.emit(&mut decoded, node)? {
            After::Drop => Ok(()),
            After::Same => self.visit_children(&node.children),
            After::Child => {
                if node.children.is_empty() {
                    return Ok(());
                }
                self.builder.push_scope()?;
                let r = self.visit_children(&node.children);
                self.builder.pop_scope();
                r
            }
            After::Defaults => {
                self.visit_defaults(&node.children);
                Ok(())
            }
        }
    }

    /// `TAG_CURRENTATTRIBUTES`' children are the editor's *current*
    /// attributes: what the original applies to the next object the user
    /// draws (`Kernel/rechdoc.cpp:1942-1966` reads them in a "set current
    /// attribute" insert mode). They are **not** the document's default
    /// attribute block, which is fixed at start-up and never written to a
    /// file (`docs/research/02-document-model.md` §4.4). An object with no
    /// fill in the file inherits the factory default, "no colour", never the
    /// current fill; treating these as defaults filled every unfilled frame
    /// with the current colour (XARA-T-0037).
    ///
    /// The model has no current-attribute store yet (the drawing tools own
    /// it), so they are counted, compared with the defaults and skipped.
    /// Definitions inside the block (colours) are still registered: later
    /// records may reference them.
    fn visit_defaults(&mut self, children: &[RecordNode]) {
        for child in children {
            // A default is a single record; a subtree under one is never
            // visited, so it is accounted for as skipped.
            self.skipped = self.skipped.saturating_add(count_subtree(&child.children));
            let rec = &child.record;
            let at = (rec.number, rec.tag);
            let Ok(d) = decode(rec.tag, &rec.data, self.origin, &mut self.diags, at) else {
                self.skipped = self.skipped.saturating_add(1);
                continue;
            };
            if self.definition(&d, rec) {
                self.mapped = self.mapped.saturating_add(1);
                continue;
            }
            self.skipped = self.skipped.saturating_add(1);
            if let Some(v) = self.attribute(&d, at) {
                self.current_attributes = self.current_attributes.saturating_add(1);
                let differs = v
                    .slot()
                    .is_some_and(|s| **self.builder.document().defaults.get(s) != v);
                if differs {
                    self.current_differing = self.current_differing.saturating_add(1);
                }
            }
        }
    }

    // ── One record ──────────────────────────────────────────────────────────

    fn emit(&mut self, d: &mut Decoded, node: &RecordNode) -> Result<After, XarError> {
        let rec = &node.record;
        let at = (rec.number, rec.tag);

        // A definition is registered wherever it appears — including inside
        // a `TAG_CURRENTATTRIBUTES` block, where several files put the
        // colour their default fill then references.
        if self.definition(d, rec) {
            self.mapped = self.mapped.saturating_add(1);
            return Ok(After::Same);
        }

        // Attributes next: they are much the commonest thing in a file.
        if let Some(v) = self.attribute(d, at) {
            self.mapped = self.mapped.saturating_add(1);
            self.builder.attribute(v)?;
            return Ok(After::Same);
        }

        Ok(match d {
            // Structure the physical layer has already acted on.
            Decoded::Up
            | Decoded::Down
            | Decoded::EndOfFile
            | Decoded::AtomicTags(_)
            | Decoded::EssentialTags(_)
            | Decoded::StartCompression(_)
            | Decoded::EndCompression { .. }
            | Decoded::Header(_) => {
                self.mapped = self.mapped.saturating_add(1);
                After::Same
            }

            // The builder's root *is* the document node, so `TAG_DOCUMENT`
            // creates nothing and its children stay at the root level.
            Decoded::Document => {
                self.mapped = self.mapped.saturating_add(1);
                After::Same
            }
            Decoded::Chapter => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder.node(NodeKind::Chapter)?;
                After::Child
            }
            Decoded::Spread => {
                self.emit_spread(node)?;
                After::Drop
            }
            Decoded::Layer => {
                self.mapped = self.mapped.saturating_add(1);
                let layer = layer_from(&node.children);
                self.builder.node(NodeKind::Layer(Box::new(layer)))?;
                After::Child
            }
            Decoded::Group => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder.node(NodeKind::Group(Box::default()))?;
                After::Child
            }

            // Records consumed by the node that encloses them.
            Decoded::SpreadInformation(_)
            | Decoded::LayerDetails { .. }
            | Decoded::LayerFrameProps { .. }
            | Decoded::SpreadAnimProps(_)
            | Decoded::PathFlags(_)
            | Decoded::TextWordWrap { .. }
            | Decoded::TextIndents { .. }
            | Decoded::GridRulerSettings { .. }
            | Decoded::GridRulerOrigin(_) => {
                self.mapped = self.mapped.saturating_add(1);
                After::Same
            }

            Decoded::Path { path, style } => {
                self.mapped = self.mapped.saturating_add(1);
                // The decoded record is dropped after this; take its path
                // rather than copy three vectors per path record.
                let mut path = std::mem::take(path);
                if let Some(flags) = path_flags_of(&node.children) {
                    apply_path_flags(&mut path, flags, &mut self.diags, at);
                }
                self.builder.node(NodeKind::Path(Box::new(PathNode {
                    data: Arc::new(path),
                    filled: style.filled,
                    stroked: style.stroked,
                })))?;
                After::Child
            }
            Decoded::RegularShape(s) => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder
                    .node(NodeKind::QuickShape(Box::new(quick_shape(s))))?;
                After::Child
            }
            Decoded::NodeBitmap { corners, bitmap } => {
                match self.bitmap(*bitmap, at) {
                    Some(image) => {
                        self.mapped = self.mapped.saturating_add(1);
                        let origin = corners.first().copied().unwrap_or(Point::ORIGIN);
                        let p1 = corners.get(1).copied().unwrap_or(origin);
                        let p2 = corners.get(2).copied().unwrap_or(origin);
                        // `p0 → p1` is the width and `p1 → p2` the height
                        // (`research/01 §4.5`). Saturating, because the
                        // corners come straight out of the file.
                        let major = Vector::new(
                            p1.x.saturating_sub(origin.x),
                            p1.y.saturating_sub(origin.y),
                        );
                        let minor =
                            Vector::new(p2.x.saturating_sub(p1.x), p2.y.saturating_sub(p1.y));
                        self.builder
                            .node(NodeKind::Bitmap(Box::new(xarast_doc::BitmapNode {
                                image,
                                origin,
                                major,
                                minor,
                            })))?;
                    }
                    // `opaque_node` counts the record itself; counting it
                    // here too would map it twice.
                    None => {
                        self.opaque_node(rec)?;
                    }
                }
                After::Child
            }

            Decoded::TextStory(s) => {
                if self.opts.skip_text {
                    self.skipped = self
                        .skipped
                        .saturating_add(count_subtree(std::slice::from_ref(node)));
                    return Ok(After::Drop);
                }
                self.mapped = self.mapped.saturating_add(1);
                self.builder.node(NodeKind::TextStory(Box::new(text_story(
                    s,
                    rec.tag,
                    &node.children,
                ))))?;
                After::Child
            }
            Decoded::TextLine => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder.node(NodeKind::TextLine(Box::default()))?;
                After::Child
            }
            Decoded::TextString(s) => {
                self.mapped = self.mapped.saturating_add(1);
                for ch in s.chars() {
                    self.builder.node(NodeKind::TextItem(TextItem::Char(ch)))?;
                }
                After::Same
            }
            Decoded::TextChar(u) => {
                self.mapped = self.mapped.saturating_add(1);
                let ch = char::from_u32(u32::from(*u)).unwrap_or('\u{fffd}');
                self.builder.node(NodeKind::TextItem(TextItem::Char(ch)))?;
                After::Same
            }
            Decoded::TextEol => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder
                    .node(NodeKind::TextItem(TextItem::LineBreak(true)))?;
                After::Same
            }
            Decoded::TextTab => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder.node(NodeKind::TextItem(TextItem::Tab))?;
                After::Same
            }
            Decoded::TextKern(v) => {
                self.mapped = self.mapped.saturating_add(1);
                self.builder
                    .node(NodeKind::TextItem(TextItem::Kern(v.dx)))?;
                After::Same
            }
            // Line metrics are Phase 9's derived cache, not node data.
            Decoded::TextLineInfo { .. } => {
                self.skipped = self.skipped.saturating_add(1);
                After::Same
            }

            Decoded::DocumentDates {
                created,
                last_saved,
            } => {
                self.mapped = self.mapped.saturating_add(1);
                let mut meta = self.builder.document().meta.clone();
                meta.created = Some(i64::from(*created));
                meta.modified = Some(i64::from(*last_saved));
                self.builder.meta(meta);
                After::Same
            }

            Decoded::CurrentAttributes(_) => {
                self.mapped = self.mapped.saturating_add(1);
                After::Defaults
            }

            // Read, understood and deliberately dropped: view state,
            // printing, units, editor hints.
            Decoded::Viewport(_)
            | Decoded::DocumentView { .. }
            | Decoded::DefaultUnits { .. }
            | Decoded::DocumentUndoSize(_)
            | Decoded::DocumentFlags(_)
            | Decoded::DocumentNudge(_)
            | Decoded::BitmapSmoothing(_)
            | Decoded::DuplicationOffset(_)
            | Decoded::BarProperty(_)
            | Decoded::SetSentinel
            | Decoded::SpreadScaling { .. }
            | Decoded::Bounds(_)
            | Decoded::BitmapProperties { .. }
            | Decoded::PreviewBitmap(_) => {
                self.skipped = self.skipped.saturating_add(1);
                After::Same
            }

            Decoded::Unhandled { tag, .. } if class_of(*tag) == Some(TagClass::Ignorable) => {
                self.skipped = self.skipped.saturating_add(1);
                After::Same
            }

            // `Decoded` is `#[non_exhaustive]`; a variant added later without
            // a mapping is kept rather than silently lost.
            _ => {
                self.opaque_node(rec)?;
                After::Child
            }
        })
    }

    /// Registers a colour, bitmap or font definition. Returns whether the
    /// record was one.
    fn definition(&mut self, d: &Decoded, rec: &crate::Record) -> bool {
        match d {
            Decoded::ColourDefinition(c) => {
                let builder = &mut self.builder;
                self.colours
                    .define_external(rec.number, c, &mut self.diags, |def| {
                        builder.define_colour(def)
                    });
            }
            Decoded::BitmapDefinition(b) => {
                let bytes: &[u8] = rec.data.get(b.image.clone()).unwrap_or(&[]);
                let original = if self.opts.skip_bitmaps {
                    None
                } else {
                    Some(Arc::new(OriginalEncoded {
                        format: image_format(b.format),
                        bytes: Arc::from(bytes),
                    }))
                };
                let id = self.builder.define_bitmap(BitmapResource {
                    name: Arc::from(b.name.as_str()),
                    info: BitmapInfo::default(),
                    pixels: Arc::new(BitmapData::default()),
                    original,
                    procedural: None,
                    transparent_index: None,
                });
                self.bitmap_by_record.insert(rec.number, id);
            }
            Decoded::FontDefinition(f) => {
                self.font_by_record.insert(
                    rec.number,
                    Arc::new(TypefaceRef {
                        full_name: Arc::from(f.full_name.as_str()),
                        family: Arc::from(f.typeface.as_str()),
                        panose: Some(f.panose),
                    }),
                );
            }
            _ => return false,
        }
        true
    }

    fn opaque_node(&mut self, rec: &crate::Record) -> Result<(), XarError> {
        self.opaque = self.opaque.saturating_add(1);
        self.mapped = self.mapped.saturating_add(1);
        self.builder.node(NodeKind::Opaque(Box::new(OpaqueNode {
            tag: rec.tag,
            payload: Arc::from(rec.data.as_slice()),
        })))?;
        Ok(())
    }

    // ── Structure ───────────────────────────────────────────────────────────

    fn emit_spread(&mut self, node: &RecordNode) -> Result<(), XarError> {
        self.mapped = self.mapped.saturating_add(1);
        let info = spread_information_of(&node.children);
        let anim = anim_of(&node.children);
        let rect = info.as_ref().map_or_else(
            || SpreadNode::default().page_size,
            |i| {
                self.origin = spread_origin(i);
                pages_rect(i)
            },
        );
        let spread = SpreadNode {
            page_size: rect,
            margin: info.as_ref().map_or(Mp::ZERO, page_margin),
            bleed: info.as_ref().map_or(Mp::ZERO, |i| i.bleed),
            double_page: info.is_some_and(|i| i.double_page_spread()),
            show_shadow: info.is_some_and(|i| i.show_page_shadow()),
            anim,
        };
        let double = spread.double_page;
        self.builder.node(NodeKind::Spread(Box::new(spread)))?;
        if node.children.is_empty() {
            return Ok(());
        }
        self.builder.push_scope()?;
        // `.xar` has no page record: the pages are implied by the spread's
        // size and its double-page flag.
        if double {
            let mid = rect.lo.x.midpoint(rect.hi.x);
            self.builder.node(NodeKind::Page(Box::new(PageNode {
                rect: Rect::new(rect.lo, Point::new(mid, rect.hi.y)),
                right_hand: false,
            })))?;
            self.builder.node(NodeKind::Page(Box::new(PageNode {
                rect: Rect::new(Point::new(mid, rect.lo.y), rect.hi),
                right_hand: true,
            })))?;
        } else {
            self.builder.node(NodeKind::Page(Box::new(PageNode {
                rect,
                right_hand: false,
            })))?;
        }
        let r = self.visit_children(&node.children);
        self.builder.pop_scope();
        r
    }

    fn emit_grid(&mut self, siblings: &[RecordNode]) -> Result<(), XarError> {
        let mut grid = GridNode::default();
        for s in siblings {
            let rec = &s.record;
            if !matches!(rec.tag, 46 | 47) {
                continue;
            }
            let at = (rec.number, rec.tag);
            match decode(rec.tag, &rec.data, self.origin, &mut self.diags, at) {
                Ok(Decoded::GridRulerSettings {
                    unit,
                    divisions,
                    subdivisions,
                    kind,
                }) => {
                    grid.kind = if kind == 1 {
                        GridKind::Isometric
                    } else {
                        GridKind::Rect
                    };
                    // `divisions` is divisions **per unit**, and the unit is
                    // the record's own reference; the model stores the
                    // spacing of one division, in millipoints. Reading the
                    // unit matters: the corpus's grids are in pixels, where
                    // assuming points would make every grid a third too
                    // coarse.
                    let per_unit = match unit {
                        Ref::Builtin(v) => builtin_unit_mp(v),
                        _ => None,
                    };
                    if let Some(per_unit) = per_unit
                        && divisions.is_finite()
                        && divisions > 0.0
                    {
                        grid.spacing = Mp::from_f64_round(per_unit / divisions);
                    }
                    grid.subdivisions = subdivisions.min(1024);
                }
                Ok(Decoded::GridRulerOrigin(p)) => grid.origin = p,
                _ => {}
            }
        }
        self.builder.node(NodeKind::Grid(Box::new(grid)))?;
        Ok(())
    }

    // ── Attributes ──────────────────────────────────────────────────────────

    /// The [`AttrValue`] a record carries, if it carries one.
    ///
    /// The tag-to-slot table lives in `xarast_doc::attr::tags`; this function
    /// produces the *value*, and a test checks the two agree on every tag the
    /// corpus contains.
    fn attribute(&mut self, d: &Decoded, at: (u32, u32)) -> Option<AttrValue> {
        Some(match d {
            Decoded::FlatFill(r) => {
                let c = self.colour(*r, at);
                AttrValue::Fill(Paint::Flat { value: c })
            }
            Decoded::LineColour(r) => {
                let c = self.colour(*r, at);
                AttrValue::StrokeColour(Paint::Flat { value: c })
            }
            Decoded::LineWidth(w) => AttrValue::LineWidth(*w),
            Decoded::Gradient(g) => AttrValue::Fill(self.paint(g, at)),
            Decoded::FlatTransparency { level, mode } => AttrValue::TranspFill(TranspPaint::Flat {
                value: Transparency {
                    level: *level,
                    mode: *mode,
                },
            }),
            Decoded::GradientTransparency(g) => AttrValue::TranspFill(self.transp_paint(g, at)),
            Decoded::LineTransparency { level, mode } => {
                AttrValue::StrokeTransp(TranspPaint::Flat {
                    value: Transparency {
                        level: *level,
                        mode: *mode,
                    },
                })
            }
            // The format stores the two caps separately and can express a
            // mismatched pair; the model, like the original, has one cap.
            Decoded::StartCap(v) | Decoded::EndCap(v) => AttrValue::LineCap(cap(*v)),
            Decoded::JoinStyle(v) => AttrValue::JoinType(join(*v)),
            Decoded::MitreLimit(v) => AttrValue::MitreLimit(*v),
            Decoded::WindingRule(v) => AttrValue::WindingRule(FillRule::from_byte(*v)),
            Decoded::Quality(v) => AttrValue::Quality(quality(*v)),
            Decoded::FillMapping(r) => AttrValue::FillMapping(tiling(*r)),
            Decoded::TransparencyMapping(r) => AttrValue::TranspFillMapping(tiling(*r)),
            Decoded::FillEffect(e) => AttrValue::FillEffect(match e {
                FillEffect::Fade => xarast_color::FillEffect::Fade,
                FillEffect::Rainbow => xarast_color::FillEffect::Rainbow,
                FillEffect::AltRainbow => xarast_color::FillEffect::AltRainbow,
            }),
            Decoded::DashStyle(r) => AttrValue::DashPattern(Arc::new(self.dash(*r, at)?)),
            Decoded::DefineDash {
                start,
                width,
                elements,
                scaled,
            } => AttrValue::DashPattern(Arc::new(DashPattern {
                elements: elements.clone(),
                offset: *start,
                reference_width: scaled.then_some(*width),
            })),
            Decoded::Arrow {
                head,
                width,
                height,
                ..
            } => {
                let spec = Arc::new(ArrowSpec {
                    name: None,
                    path: None,
                    width: *width as f32,
                    height: *height as f32,
                });
                if *head {
                    AttrValue::StartArrow(spec)
                } else {
                    AttrValue::EndArrow(spec)
                }
            }
            Decoded::Feather { size, bias, gain } => AttrValue::Feather {
                size: *size,
                profile: BiasGain::new(*bias, *gain),
            },
            Decoded::TextAttr(a) => self.text_attribute(*a, at)?,
            _ => return None,
        })
    }

    fn text_attribute(&mut self, a: TextAttr, at: (u32, u32)) -> Option<AttrValue> {
        use xarast_doc::{Justification, LineSpacing, Script};
        Some(match a {
            TextAttr::LineSpaceRatio(v) => AttrValue::LineSpace(LineSpacing::Ratio(v as f32)),
            TextAttr::LineSpaceAbsolute(v) => AttrValue::LineSpace(LineSpacing::Absolute(v)),
            TextAttr::Justification(v) => AttrValue::Justification(match v {
                1 => Justification::Centre,
                2 => Justification::Right,
                3 => Justification::Full,
                _ => Justification::Left,
            }),
            TextAttr::FontSize(v) => AttrValue::FontSize(v),
            TextAttr::Typeface(r) => AttrValue::FontTypeface(self.typeface(r, at)),
            TextAttr::Bold(v) => AttrValue::Bold(v),
            TextAttr::Italic(v) => AttrValue::Italic(v),
            TextAttr::Underline(v) => AttrValue::Underline(v),
            TextAttr::Script { offset, size } => AttrValue::Script(Script {
                on: true,
                offset: offset as f32,
                size: size as f32,
            }),
            TextAttr::ScriptOff => AttrValue::Script(Script::default()),
            // The implicit forms; the original picks the same ratios.
            TextAttr::Superscript => AttrValue::Script(Script {
                on: true,
                offset: 0.33,
                size: 0.6,
            }),
            TextAttr::Subscript => AttrValue::Script(Script {
                on: true,
                offset: -0.15,
                size: 0.6,
            }),
            // Carried through unconverted: the effective unit is still open
            // (`research/01 §11` item 15, `docs/memory/xar-import.md`).
            TextAttr::Tracking(v) => AttrValue::Tracking(Mp::new(v)),
            TextAttr::AspectRatio(v) => AttrValue::AspectRatio(v as f32),
            TextAttr::Baseline(v) => AttrValue::Baseline(v),
            TextAttr::LeftIndent(v) => AttrValue::LeftMargin(v),
            TextAttr::FirstIndent(v) => AttrValue::FirstIndent(v),
            TextAttr::RightIndent(v) => AttrValue::RightMargin(v),
        })
    }

    fn typeface(&mut self, r: Ref, at: (u32, u32)) -> Arc<TypefaceRef> {
        if let Ref::Record(n) = r
            && let Some(f) = self.font_by_record.get(&n)
        {
            return Arc::clone(f);
        }
        if r != Ref::None {
            self.diags.push(
                Diagnostic::new(DiagCode::DanglingReference)
                    .at(at.0, at.1)
                    .with_detail(match r {
                        Ref::Record(n) => u64::from(n),
                        Ref::Builtin(v) => u64::from(v.unsigned_abs()),
                        Ref::None => 0,
                    }),
            );
        }
        Arc::new(TypefaceRef {
            full_name: Arc::from("Times New Roman"),
            family: Arc::from("Times New Roman"),
            panose: None,
        })
    }

    /// The predefined dash patterns of `Kernel/cxfdash.h` are numbers, not
    /// geometry: their element lists live in the original's dash gallery and
    /// are not in the format. Rather than invent twenty patterns, an
    /// unresolvable reference produces no attribute at all, so the line keeps
    /// whatever it inherited, and says so.
    fn dash(&mut self, r: Ref, at: (u32, u32)) -> Option<DashPattern> {
        match r {
            // −21 is "solid", which is exactly the empty pattern.
            Ref::None | Ref::Builtin(-21) => Some(DashPattern::default()),
            Ref::Builtin(v) => {
                self.diags.push(
                    Diagnostic::new(DiagCode::UnknownEnumValue)
                        .at(at.0, at.1)
                        .with_detail(u64::from(v.unsigned_abs())),
                );
                None
            }
            Ref::Record(n) => {
                self.diags.push(
                    Diagnostic::new(DiagCode::DanglingReference)
                        .at(at.0, at.1)
                        .with_detail(u64::from(n)),
                );
                None
            }
        }
    }

    fn bitmap(&mut self, r: Ref, at: (u32, u32)) -> Option<BitmapId> {
        match r {
            Ref::Record(n) => {
                let found = self.bitmap_by_record.get(&n).copied();
                if found.is_none() {
                    self.diags.push(
                        Diagnostic::new(DiagCode::DanglingReference)
                            .at(at.0, at.1)
                            .with_detail(u64::from(n)),
                    );
                }
                found
            }
            _ => None,
        }
    }

    /// A colour reference as a paintable value.
    ///
    /// "No colour" is the fully transparent one: the model's [`Paint`] is
    /// always a colour, and `TAG_FLATFILL_NONE` means *do not paint*, which a
    /// transparent flat fill expresses exactly.
    fn colour(&mut self, r: Ref, at: (u32, u32)) -> Colour {
        self.colours
            .resolve(r, &mut self.diags, at)
            .unwrap_or(Colour::Direct(ColourValue::rgbt(0.0, 0.0, 0.0, 1.0)))
    }

    // ── Fills ───────────────────────────────────────────────────────────────

    fn paint(&mut self, g: &GradientFill, at: (u32, u32)) -> Paint {
        let c = |m: &mut Mapper<'o>, i: usize| -> Colour {
            let r = g.colours.get(i).copied().unwrap_or(Ref::None);
            m.colour(r, at)
        };
        let from = c(self, 0);
        let to = c(self, 1);
        let mut ramp: Ramp<Colour> = Ramp::new();
        for (pos, cref) in &g.ramp {
            let value = self.colour(*cref, at);
            ramp.insert(RampStop {
                pos: *pos as f32,
                value,
            });
        }
        ramp.profile = BiasGain::new(g.profile.0, g.profile.1);
        let geo = &g.geometry;
        match g.kind {
            FillKind::Linear | FillKind::Linear3Point => {
                if g.kind == FillKind::Linear3Point {
                    self.unsupported(at);
                }
                Paint::Linear {
                    start: geo.start,
                    end: geo.end,
                    persp: None,
                    from,
                    to,
                    ramp,
                }
            }
            FillKind::Circular | FillKind::Elliptical => Paint::Radial {
                centre: geo.start,
                major: geo.end,
                minor: geo.end2.unwrap_or(geo.end),
                aspect_locked: g.kind == FillKind::Circular,
                persp: None,
                from,
                to,
                ramp,
            },
            FillKind::Conical => Paint::Conical {
                centre: geo.start,
                zero_dir: geo.end,
                from,
                to,
                ramp,
            },
            FillKind::Square => Paint::Diamond {
                centre: geo.start,
                corner1: geo.end,
                corner2: geo.end2.unwrap_or(geo.end),
                persp: None,
                from,
                to,
                ramp,
            },
            FillKind::ThreeColour => Paint::ThreeColour {
                origin: geo.start,
                axis1: geo.end,
                axis2: geo.end2.unwrap_or(geo.end),
                c0: from,
                c1: to,
                c2: c(self, 2),
            },
            FillKind::FourColour => {
                let axis2 = geo.end2.unwrap_or(geo.end);
                Paint::FourColour {
                    origin: geo.start,
                    axis1: geo.end,
                    axis2,
                    axis3: opposite_corner(geo.start, geo.end, axis2),
                    c0: from,
                    c1: to,
                    c2: c(self, 2),
                    c3: c(self, 3),
                }
            }
            FillKind::Bitmap | FillKind::ContoneBitmap => {
                let contone = (g.kind == FillKind::ContoneBitmap).then_some((from, to));
                match g.bitmap.and_then(|r| self.bitmap(r, at)) {
                    Some(image) => Paint::Bitmap {
                        image,
                        origin: geo.start,
                        axis_x: geo.end,
                        axis_y: geo.end2.unwrap_or(geo.end),
                        persp: None,
                        tiling: Tiling::default(),
                        dpi: 96,
                        contone,
                        profile: BiasGain::new(g.profile.0, g.profile.1),
                    },
                    None => Paint::Flat {
                        value: Colour::Direct(ColourValue::rgbt(0.0, 0.0, 0.0, 1.0)),
                    },
                }
            }
            FillKind::Fractal => Paint::Fractal {
                params: Box::new(procedural(g.fractal.as_ref())),
                from,
                to,
                profile: BiasGain::new(g.profile.0, g.profile.1),
            },
            FillKind::Noise => Paint::Noise {
                params: Box::new(procedural(g.fractal.as_ref())),
                from,
                to,
                profile: BiasGain::new(g.profile.0, g.profile.1),
            },
        }
    }

    fn transp_paint(&mut self, g: &GradientTransparency, at: (u32, u32)) -> TranspPaint {
        let t = |i: usize| Transparency {
            level: g.levels.get(i).copied().unwrap_or(0),
            mode: g.mode,
        };
        let from = t(0);
        let to = t(1);
        let mut ramp: Ramp<Transparency> = Ramp::new();
        ramp.profile = BiasGain::new(g.profile.0, g.profile.1);
        let geo = &g.geometry;
        match g.kind {
            FillKind::Linear | FillKind::Linear3Point => {
                if g.kind == FillKind::Linear3Point {
                    self.unsupported(at);
                }
                TranspPaint::Linear {
                    start: geo.start,
                    end: geo.end,
                    persp: None,
                    from,
                    to,
                    ramp,
                }
            }
            FillKind::Circular | FillKind::Elliptical => TranspPaint::Radial {
                centre: geo.start,
                major: geo.end,
                minor: geo.end2.unwrap_or(geo.end),
                aspect_locked: g.kind == FillKind::Circular,
                persp: None,
                from,
                to,
                ramp,
            },
            FillKind::Conical => TranspPaint::Conical {
                centre: geo.start,
                zero_dir: geo.end,
                from,
                to,
                ramp,
            },
            FillKind::Square => TranspPaint::Diamond {
                centre: geo.start,
                corner1: geo.end,
                corner2: geo.end2.unwrap_or(geo.end),
                persp: None,
                from,
                to,
                ramp,
            },
            FillKind::ThreeColour => TranspPaint::ThreeColour {
                origin: geo.start,
                axis1: geo.end,
                axis2: geo.end2.unwrap_or(geo.end),
                c0: from,
                c1: to,
                c2: t(2),
            },
            FillKind::FourColour => {
                let axis2 = geo.end2.unwrap_or(geo.end);
                TranspPaint::FourColour {
                    origin: geo.start,
                    axis1: geo.end,
                    axis2,
                    axis3: opposite_corner(geo.start, geo.end, axis2),
                    c0: from,
                    c1: to,
                    c2: t(2),
                    c3: t(3),
                }
            }
            FillKind::Bitmap | FillKind::ContoneBitmap => {
                match g.bitmap.and_then(|r| self.bitmap(r, at)) {
                    Some(image) => TranspPaint::Bitmap {
                        image,
                        origin: geo.start,
                        axis_x: geo.end,
                        axis_y: geo.end2.unwrap_or(geo.end),
                        persp: None,
                        tiling: Tiling::default(),
                        dpi: 96,
                        contone: None,
                        profile: BiasGain::new(g.profile.0, g.profile.1),
                    },
                    None => TranspPaint::Flat { value: from },
                }
            }
            FillKind::Fractal => TranspPaint::Fractal {
                params: Box::new(procedural(g.fractal.as_ref())),
                from,
                to,
                profile: BiasGain::new(g.profile.0, g.profile.1),
            },
            FillKind::Noise => TranspPaint::Noise {
                params: Box::new(procedural(g.fractal.as_ref())),
                from,
                to,
                profile: BiasGain::new(g.profile.0, g.profile.1),
            },
        }
    }

    fn unsupported(&mut self, at: (u32, u32)) {
        self.diags.push(
            Diagnostic::new(DiagCode::UnknownEnumValue)
                .at(at.0, at.1)
                .with_severity(Severity::Info),
        );
    }
}

// ── Free helpers ────────────────────────────────────────────────────────────

/// How many records a subtree holds, counting its root.
fn count_subtree(nodes: &[RecordNode]) -> u32 {
    let mut n = 0u32;
    for node in nodes {
        n = n
            .saturating_add(1)
            .saturating_add(count_subtree(&node.children));
    }
    n
}

/// Millipoints per predefined unit, for the negative unit references of
/// `research/01 §5.2`. `None` for "untyped" and for anything unknown, which
/// leaves the caller's default in place rather than inventing a scale.
fn builtin_unit_mp(v: i32) -> Option<f64> {
    Some(match v {
        -2 => 2_834.652_715,    // millimetre
        -3 => 28_346.527_15,    // centimetre
        -4 => 2_834_652.715,    // metre
        -5 => 2_834_652_715.0,  // kilometre
        -6 => 1.0,              // millipoint
        -7 => 1_000.0,          // point
        -8 => 12_000.0,         // pica
        -9 => 72_000.0,         // inch
        -10 => 864_000.0,       // foot
        -11 => 2_592_000.0,     // yard
        -12 => 4_561_920_000.0, // mile
        -13 => 750.0,           // pixel at 96 dpi
        _ => return None,
    })
}

/// The fourth corner of the parallelogram `origin`, `a`, `b`.
fn opposite_corner(origin: Point, a: Point, b: Point) -> Point {
    Point::new(
        a.x.saturating_add(b.x).saturating_sub(origin.x),
        a.y.saturating_add(b.y).saturating_sub(origin.y),
    )
}

fn procedural(p: Option<&FractalParams>) -> ProceduralParams {
    match p {
        Some(f) => ProceduralParams {
            seed: f.seed,
            graininess: f.graininess as f32,
            gravity: f.gravity as f32,
            squash: f.squash as f32,
            dpi: u32::try_from(f.dpi).unwrap_or(96),
            tileable: f.tileable,
        },
        None => ProceduralParams::default(),
    }
}

fn cap(v: u8) -> Cap {
    match v {
        2 => Cap::Round,
        3 => Cap::Square,
        _ => Cap::Butt,
    }
}

fn join(v: u8) -> Join {
    match v {
        2 => Join::Round,
        3 => Join::Bevel,
        _ => Join::Mitre,
    }
}

/// `TAG_QUALITY` is a 0..110 slider, not an enum.
///
/// The original derives what to draw from thresholds
/// (`Kernel/quality.cpp:157-255`): above 30 lines are real rather than
/// black hairlines, from 60 fills are graduated, and above 100 rendering is
/// antialiased. Those three thresholds are exactly the model's four levels.
fn quality(v: i32) -> xarast_doc::Quality {
    use xarast_doc::Quality as Q;
    match v {
        i32::MIN..=30 => Q::Outline,
        31..=59 => Q::Simple,
        60..=100 => Q::Normal,
        _ => Q::Full,
    }
}

/// The mapping records keep the original's attribute values: a
/// non-repeating record reads back as 1 (`Tiling::Simple`), and the "extra"
/// repeat (206/207) as 4, which is the only value that makes a graduated
/// fill tile. How each fill family interprets them is the walker's business
/// (`docs/research/01-xar-format.md` §8.3).
fn tiling(r: crate::decode::FillRepeat) -> Tiling {
    use crate::decode::FillRepeat as R;
    match r {
        R::None => Tiling::Simple,
        R::Repeat => Tiling::Repeat,
        R::RepeatInverted => Tiling::RepeatInverted,
        R::Extra => Tiling::RepeatExtra,
    }
}

fn image_format(f: crate::decode::BitmapFormat) -> ImageFormat {
    use crate::decode::BitmapFormat as B;
    match f {
        B::Png => ImageFormat::Png,
        B::Jpeg | B::Jpeg8Bpp => ImageFormat::Jpeg,
        B::Gif => ImageFormat::Gif,
        B::Bmp => ImageFormat::Bmp,
        B::BmpZip => ImageFormat::Unknown,
    }
}

/// The record's two paths are edge *templates*, not the outline
/// (`research/01 §4.7.1`). The outline is generated here, in the shape's own
/// untransformed space where the record's parameters are exact, and only then
/// carried through the matrix.
fn quick_shape(s: &RegularShape) -> QuickShape {
    use crate::decode::ShapeFlags as F;
    let m = s.matrix;
    // A two-point template is a straight edge, which is also the default, so
    // only a curved template is worth keeping.
    let template = |p: &xarast_geom::Path| (p.points().len() > 2).then(|| Arc::new(p.clone()));
    let spec = xarast_geom::RegularShapeSpec {
        sides: u32::from(s.sides),
        circular: s.flags.contains(F::CIRCULAR),
        stellated: s.flags.contains(F::STELLATED),
        primary_curved: s.flags.contains(F::PRIMARY_CURVATURE),
        stellation_curved: s.flags.contains(F::STELLATION_CURVATURE),
        centre: Point::ORIGIN,
        major: s.major_axis,
        minor: s.minor_axis,
        stellation_radius: s.stellation_radius,
        stellation_offset: s.stellation_offset,
        primary_curvature: s.primary_curvature,
        stellation_curvature: s.secondary_curvature,
        primary_edge: Some(&s.primary_edge),
        secondary_edge: Some(&s.secondary_edge),
    };
    let path = xarast_geom::regular_shape_outline(&spec).map(|p| Arc::new(p.transformed(m)));
    QuickShape {
        sides: spec.sides,
        circular: spec.circular,
        stellated: spec.stellated,
        curved: spec.primary_curved,
        stellation_curved: spec.stellation_curved,
        centre: m.transform_point(Point::ORIGIN),
        major: m.transform_vector(s.major_axis),
        minor: m.transform_vector(s.minor_axis),
        stellation_radius: s.stellation_radius,
        stellation_offset: s.stellation_offset,
        primary_curvature: s.primary_curvature,
        stellation_curvature: s.secondary_curvature,
        primary_edge: template(&s.primary_edge),
        secondary_edge: template(&s.secondary_edge),
        path,
    }
}

/// `TAG_PATH_FLAGS` is written as the path's **first child**
/// (`research/01 §7.5`), so the path reads it before it is emitted.
fn path_flags_of(children: &[RecordNode]) -> Option<&[u8]> {
    children
        .iter()
        .find(|c| c.record.tag == 111)
        .map(|c| c.record.data.as_slice())
}

fn spread_information_of(children: &[RecordNode]) -> Option<SpreadInformation> {
    let rec = &children.iter().find(|c| c.record.tag == 45)?.record;
    let mut sink = DiagSink::new();
    match decode(
        rec.tag,
        &rec.data,
        Point::ORIGIN,
        &mut sink,
        (rec.number, rec.tag),
    ) {
        Ok(Decoded::SpreadInformation(i)) if i.width.raw() > 0 && i.height.raw() > 0 => Some(i),
        _ => None,
    }
}

fn anim_of(children: &[RecordNode]) -> Option<Box<xarast_doc::AnimProps>> {
    let rec = &children.iter().find(|c| c.record.tag == 4031)?.record;
    let mut sink = DiagSink::new();
    let Ok(Decoded::SpreadAnimProps(w)) = decode(
        rec.tag,
        &rec.data,
        Point::ORIGIN,
        &mut sink,
        (rec.number, rec.tag),
    ) else {
        return None;
    };
    // Seven words: loop, global_delay, dither, web_palette, colours_palette,
    // num_colours, flags (`research/01 §4.3`). `AnimProps` has a home for
    // exactly one of them — the delay — because the other six describe how
    // the spread is *exported* as a GIF rather than what it is. Recorded in
    // `docs/memory/xar-import.md` as a model gap rather than crammed in.
    Some(Box::new(xarast_doc::AnimProps {
        delay: w.get(1).copied().unwrap_or(0),
        hidden: false,
        background: false,
    }))
}

fn layer_from(children: &[RecordNode]) -> LayerNode {
    let mut layer = LayerNode {
        active: false,
        ..LayerNode::default()
    };
    let mut sink = DiagSink::new();
    for c in children {
        let rec = &c.record;
        match rec.tag {
            48 | 49 => {
                if let Ok(Decoded::LayerDetails { flags, name, .. }) = decode(
                    rec.tag,
                    &rec.data,
                    Point::ORIGIN,
                    &mut sink,
                    (rec.number, rec.tag),
                ) {
                    layer.name = Arc::from(name.as_str());
                    layer.visible = flags.contains(LayerFlags::VISIBLE);
                    layer.locked = flags.contains(LayerFlags::LOCKED);
                    layer.printable = flags.contains(LayerFlags::PRINTABLE);
                    layer.active = flags.contains(LayerFlags::ACTIVE);
                    layer.page_background = flags.contains(LayerFlags::PAGE_BACKGROUND);
                    layer.background = flags.contains(LayerFlags::BACKGROUND);
                    layer.guide = rec.tag == 49;
                }
            }
            4030 => {
                if let Ok(Decoded::LayerFrameProps { delay, flags }) = decode(
                    rec.tag,
                    &rec.data,
                    Point::ORIGIN,
                    &mut sink,
                    (rec.number, rec.tag),
                ) {
                    layer.frame = Some(Box::new(xarast_doc::FrameProps {
                        delay,
                        solid: flags & 0x01 != 0,
                        overlay: flags & 0x02 != 0,
                    }));
                }
            }
            _ => {}
        }
    }
    layer
}

fn text_story(s: &crate::decode::TextStory, tag: u32, children: &[RecordNode]) -> TextStoryNode {
    let transform = match s.placement {
        TextPlacement::Simple(p) => Matrix::translate(Vector::new(p.x, p.y)),
        TextPlacement::Complex(m) => m,
    };
    let mut sink = DiagSink::new();
    let mut column: Option<(Mp, bool)> = None;
    let mut indents = (Mp::ZERO, Mp::ZERO);
    for c in children {
        let rec = &c.record;
        let at = (rec.number, rec.tag);
        match decode(rec.tag, &rec.data, Point::ORIGIN, &mut sink, at) {
            Ok(Decoded::TextWordWrap { width, enabled }) => column = Some((width, enabled)),
            Ok(Decoded::TextIndents { left, right }) => indents = (left, right),
            _ => {}
        }
    }
    let layout = if s.on_path {
        // 2110–2113 and 2114–2117 are `{START,END}_{LEFT,RIGHT}` in that
        // order, and `research/01 §4.12.2` reads the pair as "from which
        // end, and in which direction it flows". `reversed` is therefore
        // the RIGHT half: the odd tags. Confirm against a reference
        // rendering in Phase 9; seven records in the corpus depend on it.
        TextLayout::OnPath {
            reversed: tag % 2 == 1,
            tangential: true,
            left_indent: indents.0,
            right_indent: indents.1,
        }
    } else {
        match column {
            Some((width, wrap)) if width.raw() > 0 => TextLayout::InColumn {
                width,
                word_wrap: wrap,
            },
            _ => TextLayout::AtPoint,
        }
    };
    TextStoryNode {
        transform,
        layout,
        auto_kern: s.autokern,
        print_as_shapes: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::XarBuilder;

    fn info(width: i32, height: i32, margin: i32, bleed: i32, flags: u8) -> SpreadInformation {
        SpreadInformation {
            width: Mp::new(width),
            height: Mp::new(height),
            margin: Mp::new(margin),
            bleed: Mp::new(bleed),
            flags,
        }
    }

    #[test]
    fn the_origin_is_the_pasteboard_margin() {
        let i = info(600_000, 450_000, 576_000, 0, 2);
        assert_eq!(spread_origin(&i), Point::raw(576_000, 576_000));
        assert_eq!(
            pages_rect(&i),
            Rect::new(
                Point::raw(576_000, 576_000),
                Point::raw(1_176_000, 1_026_000)
            )
        );
    }

    #[test]
    fn a_zero_margin_spread_has_a_zero_origin() {
        let i = info(1_533_750, 1_152_000, 0, 0, 0);
        assert_eq!(spread_origin(&i), Point::ORIGIN);
    }

    /// The case no corpus file exercises: `bleed > margin`, where the pages
    /// are pushed out by `margin + bleed` rather than by `margin`.
    #[test]
    fn a_bleed_larger_than_the_margin_widens_the_origin() {
        let i = info(600_000, 450_000, 10_000, 25_000, 0);
        assert_eq!(spread_origin(&i), Point::raw(35_000, 35_000));
        let j = info(600_000, 450_000, 25_000, 10_000, 0);
        assert_eq!(spread_origin(&j), Point::raw(25_000, 25_000));
    }

    #[test]
    fn a_double_page_spread_is_twice_as_wide_from_the_same_corner() {
        let i = info(600_000, 450_000, 72_000, 0, 1);
        assert_eq!(spread_origin(&i), Point::raw(72_000, 72_000));
        assert_eq!(
            pages_rect(&i),
            Rect::new(Point::raw(72_000, 72_000), Point::raw(1_272_000, 522_000))
        );
    }

    #[test]
    fn the_node_kind_names_match_the_model() {
        // `Opaque` is the last discriminant; if the model gains a variant
        // this fails rather than silently mis-labelling a column.
        let opaque = NodeKind::Opaque(Box::new(OpaqueNode {
            tag: 0,
            payload: Arc::from(&[][..]),
        }));
        assert_eq!(usize::from(opaque.discriminant()), NODE_KIND_COUNT - 1);
        assert_eq!(
            NODE_KIND_NAMES.get(usize::from(opaque.discriminant())),
            Some(&opaque.type_name())
        );
    }

    fn minimal_drawing(margin: i32) -> Vec<u8> {
        minimal_drawing_with(margin, 0)
    }

    fn minimal_drawing_with(margin: i32, bleed: i32) -> Vec<u8> {
        let mut spread = Vec::new();
        spread.extend_from_slice(&600_000i32.to_le_bytes());
        spread.extend_from_slice(&450_000i32.to_le_bytes());
        spread.extend_from_slice(&margin.to_le_bytes());
        spread.extend_from_slice(&bleed.to_le_bytes());
        spread.push(2);

        let mut layer = vec![0x01 | 0x04 | 0x08];
        for u in "Layer 1".encode_utf16() {
            layer.extend_from_slice(&u.to_le_bytes());
        }
        layer.extend_from_slice(&0u16.to_le_bytes());

        // A two-point relative path: MoveTo(1000, 2000), LineTo(500, 1500).
        let mut path = vec![0x06u8];
        path.extend_from_slice(&interleave(1000, 2000));
        path.push(0x02);
        path.extend_from_slice(&interleave(500, 500));

        XarBuilder::new()
            .record(40, &[])
            .down()
            .record(41, &[])
            .down()
            .record(42, &[])
            .down()
            .record(45, &spread)
            .record(43, &[])
            .down()
            .record(48, &layer)
            .record(116, &path)
            .down()
            .record(111, &[0x05, 0x05])
            .record(152, &500i32.to_le_bytes())
            .record(194, &[])
            .up()
            .up()
            .up()
            .up()
            .end_of_file()
            .finish()
    }

    fn interleave(x: i32, y: i32) -> [u8; 8] {
        let (x, y) = (x.to_be_bytes(), y.to_be_bytes());
        [x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]]
    }

    #[test]
    fn a_minimal_drawing_maps_into_a_valid_document() {
        let bytes = minimal_drawing(576_000);
        let (doc, report) = import(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(doc.validate().errors, Vec::new());
        assert_eq!(report.spread_origin, Point::raw(576_000, 576_000));
        assert_eq!(report.nodes_of(NodeKind::Chapter.discriminant()), 1);
        assert_eq!(
            report.nodes_of(NodeKind::Spread(Box::default()).discriminant()),
            1
        );
        assert_eq!(
            report.nodes_of(NodeKind::Page(Box::default()).discriminant()),
            1
        );
        assert_eq!(
            report.nodes_of(NodeKind::Layer(Box::default()).discriminant()),
            1
        );
        assert_eq!(report.records_opaque, 0);
    }

    #[test]
    fn a_path_lands_in_document_coordinates_with_its_flags() {
        use xarast_geom::PointFlags;
        let bytes = minimal_drawing(576_000);
        let (doc, _) = import(&bytes, &ImportOptions::default()).unwrap();
        let path = doc
            .tree
            .preorder(doc.tree.root())
            .find_map(|id| match doc.tree.kind(id) {
                Some(NodeKind::Path(p)) => Some(p.clone()),
                _ => None,
            })
            .expect("the drawing has one path");
        // Point 0 is absolute and origin-translated; point 1 is an inverted
        // delta from it, and the origin is *not* applied twice.
        assert_eq!(
            path.data.points(),
            &[
                Point::raw(1000 + 576_000, 2000 + 576_000),
                Point::raw(500 + 576_000, 1500 + 576_000)
            ]
        );
        assert_eq!(
            path.data.flags_at(0),
            PointFlags::SMOOTH | PointFlags::END_POINT
        );
        assert!(path.stroked && path.filled);
    }

    #[test]
    fn a_zero_margin_file_puts_the_page_at_the_origin() {
        let bytes = minimal_drawing(0);
        let (doc, report) = import(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(report.spread_origin, Point::ORIGIN);
        let page = doc
            .tree
            .preorder(doc.tree.root())
            .find_map(|id| match doc.tree.kind(id) {
                Some(NodeKind::Page(p)) => Some(p.rect),
                _ => None,
            })
            .expect("one page");
        assert_eq!(page.lo, Point::ORIGIN);
    }

    /// The case no corpus file exercises, as a whole file rather than as a
    /// call to [`spread_origin`]: every one of the 59 has `bleed == 0`, so
    /// nothing there can tell `margin` from
    /// `margin < bleed ? margin + bleed : margin`. This file can, and it is
    /// synthetic, so nothing of Xara's is copied to get it.
    #[test]
    fn a_synthetic_file_with_a_bleed_wider_than_its_margin_shifts_the_origin() {
        let bytes = minimal_drawing_with(10_000, 25_000);
        let (doc, report) = import(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(doc.validate().errors, Vec::new());
        // 10 000 < 25 000, so the pages are pushed out by margin + bleed.
        assert_eq!(report.spread_origin, Point::raw(35_000, 35_000));
        let page = doc
            .tree
            .preorder(doc.tree.root())
            .find_map(|id| match doc.tree.kind(id) {
                Some(NodeKind::Page(p)) => Some(p.rect),
                _ => None,
            })
            .expect("one page");
        assert_eq!(page.lo, Point::raw(35_000, 35_000));
        let path = doc
            .tree
            .preorder(doc.tree.root())
            .find_map(|id| match doc.tree.kind(id) {
                Some(NodeKind::Path(p)) => Some(p.clone()),
                _ => None,
            })
            .expect("one path");
        assert_eq!(
            path.data.points().first(),
            Some(&Point::raw(1000 + 35_000, 2000 + 35_000))
        );
    }

    #[test]
    fn an_unknown_non_ignorable_record_survives_as_opaque() {
        let bytes = XarBuilder::new()
            .record(40, &[])
            .down()
            .record(4050, &[1, 2, 3, 4])
            .up()
            .end_of_file()
            .finish();
        let (doc, report) = import(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(report.records_opaque, 1);
        let kept = doc
            .tree
            .preorder(doc.tree.root())
            .find_map(|id| match doc.tree.kind(id) {
                Some(NodeKind::Opaque(o)) => Some(o.clone()),
                _ => None,
            })
            .expect("the record was kept");
        assert_eq!(kept.tag, 4050);
        assert_eq!(&*kept.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn current_attributes_are_neither_defaults_nor_nodes() {
        // A current line width and a current black fill: what the editor
        // would give the next object drawn, not what an unattributed object
        // in the file inherits (XARA-T-0037).
        let bytes = XarBuilder::new()
            .record(40, &[])
            .down()
            .record(41, &[])
            .record(4119, &[1])
            .down()
            .record(152, &4_242i32.to_le_bytes())
            .record(191, &[])
            .up()
            .up()
            .end_of_file()
            .finish();
        let (doc, report) = import(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(report.current_attributes, 2);
        assert_eq!(report.current_differing, 2);
        let factory = xarast_doc::DefaultAttrs::xara_compatible();
        for slot in [
            xarast_doc::AttrSlot::LineWidth,
            xarast_doc::AttrSlot::FillGeometry,
        ] {
            assert_eq!(doc.defaults.get(slot), factory.get(slot), "{slot:?}");
        }
        assert_eq!(
            report.nodes_of(
                NodeKind::Attr(Box::new(xarast_doc::AttrNode::new(AttrValue::LineWidth(
                    Mp::ZERO
                ))))
                .discriminant()
            ),
            0
        );
    }

    #[test]
    fn an_empty_file_is_an_error_rather_than_an_empty_document() {
        let bytes = XarBuilder::new().end_of_file().finish();
        assert!(matches!(
            import(&bytes, &ImportOptions::default()),
            Err(XarError::Build(xarast_doc::BuildError::Empty))
        ));
    }

    #[test]
    fn a_truncated_file_still_produces_a_valid_document() {
        let full = minimal_drawing(576_000);
        for n in 16..full.len() {
            let Some(head) = full.get(..n) else { continue };
            if let Ok((doc, _)) = import(head, &ImportOptions::default()) {
                assert_eq!(doc.validate().errors, Vec::new(), "truncated at {n}");
            }
        }
    }

    /// XARA-T-0013: the record's edge paths are edge templates, not the
    /// outline. Storing one as the shape's path gave every quick shape a
    /// sliver of bounds far from where it sits, so the renderer culled it.
    /// Synthetic parameters shaped like a stellated six-pointed star, placed
    /// by a translation.
    #[test]
    fn a_quick_shape_gets_its_generated_outline_not_its_edge_template() {
        use crate::decode::ShapeFlags;
        let mut edge = xarast_geom::Path::builder();
        edge.move_to(Point::raw(-576_000, -576_000))
            .line_to(Point::raw(-504_000, -576_000));
        let edge = edge.build();
        let at = Point::raw(115_000, 301_000);
        let s = RegularShape {
            flags: ShapeFlags::STELLATED,
            sides: 6,
            major_axis: Vector::new(Mp::new(-27_750), Mp::new(47_250)),
            minor_axis: Vector::new(Mp::new(47_250), Mp::new(27_750)),
            matrix: xarast_geom::Matrix {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e: at.x,
                f: at.y,
            },
            stellation_radius: 0.5,
            stellation_offset: 0.0,
            primary_curvature: 0.2,
            secondary_curvature: 0.2,
            primary_edge: edge.clone(),
            secondary_edge: edge,
        };
        let q = quick_shape(&s);
        let path = q.path.as_deref().expect("an outline is generated");
        assert_eq!(path.points().len(), 13, "move, then twelve edges");
        let b = path.bounds();
        // The primary radius is |major| = 54 797 mp; nothing is culled.
        assert!(
            b.width().raw() > 90_000 && b.height().raw() > 90_000,
            "{b:?}"
        );
        assert!(
            b.contains(at),
            "the outline sits around the matrix translation"
        );
        for p in path.points() {
            let d = p.distance_to(at);
            assert!(
                (d - 54_797.0).abs() < 3.0 || (d - 27_398.0).abs() < 3.0,
                "every point is a primary or a stellation point: {d}"
            );
        }
        // Straight templates are the default and are not kept.
        assert!(q.primary_edge.is_none() && q.secondary_edge.is_none());
        // Regenerating from the stored parameters gives the same outline.
        assert_eq!(q.outline().as_ref(), Some(path));
    }

    /// Every quick shape is an ellipse when the circular flag is set,
    /// whatever its side count says.
    #[test]
    fn a_circular_quick_shape_is_an_ellipse_spanning_its_axes() {
        use crate::decode::ShapeFlags;
        let s = RegularShape {
            flags: ShapeFlags::CIRCULAR,
            sides: 4,
            major_axis: Vector::new(Mp::new(0), Mp::new(30_000)),
            minor_axis: Vector::new(Mp::new(20_000), Mp::new(0)),
            matrix: xarast_geom::Matrix {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e: Mp::new(500_000),
                f: Mp::new(0),
            },
            stellation_radius: 0.0,
            stellation_offset: 0.0,
            primary_curvature: 0.0,
            secondary_curvature: 0.0,
            primary_edge: xarast_geom::Path::new(),
            secondary_edge: xarast_geom::Path::new(),
        };
        let q = quick_shape(&s);
        let b = q.path.as_deref().expect("an ellipse").tight_bounds();
        assert!((b.width().raw() - 80_000).abs() <= 4, "{b:?}");
        assert!((b.height().raw() - 60_000).abs() <= 4, "{b:?}");
        assert!(b.contains(Point::raw(500_000, 0)));
    }
}
