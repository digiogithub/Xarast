//! [`NodeKind`]: the sum type that replaces the original's class hierarchy.
//!
//! `docs/10-architecture.md` §3.2 fixes this as an `enum` with exhaustive
//! `match`, not `Box<dyn Node>`. The consequence to embrace rather than work
//! around is that adding a node type makes every `match` fail to compile —
//! which is the type system listing the places that need a decision.
//!
//! The discipline that makes it pay off is **one function per behaviour, not
//! one method per variant**: [`compute_bounds`](crate::compute_bounds) and
//! [`Document::dump`](crate::Document::dump) are each a single `match`, so the
//! whole of an aspect is readable at once.
//!
//! # Boxing
//!
//! The size of the enum is set by its largest variant, and `NodeData` is gated
//! at 64 bytes. Every payload larger than a pointer is therefore `Box`ed, so
//! that a document made mostly of text items does not pay for a spread.

use std::sync::Arc;

use xarast_geom::{Mp, Path, Point, Vector};

use crate::attr::AttrNode;
use crate::live::LiveNode;
use crate::resources::BitmapId;
use crate::structure::{DocumentNode, GridNode, LayerNode, PageNode, SpreadNode};
use crate::text::{TextItem, TextLineNode, TextStoryNode};

/// What a node is.
///
/// Large variants are `Box`ed so that `NodeData` stays within a cache line.
#[derive(Clone, Debug)]
pub enum NodeKind {
    /// The root of every document.
    Document(Box<DocumentNode>),
    /// A chapter: a run of spreads. Carries nothing of its own.
    Chapter,
    /// A spread: one or two pages plus their pasteboard.
    Spread(Box<SpreadNode>),
    /// A page within a spread.
    Page(Box<PageNode>),
    /// A layer.
    Layer(Box<LayerNode>),
    /// A grid.
    Grid(Box<GridNode>),
    /// A free-form path.
    Path(Box<PathNode>),
    /// A rectangle or ellipse held as a parallelogram, as the format does.
    Shape(Box<ShapeNode>),
    /// A regular polygon or star, held as its parameters.
    QuickShape(Box<QuickShape>),
    /// A placed bitmap.
    Bitmap(Box<BitmapNode>),
    /// A guideline on a guide layer.
    Guideline(Box<GuidelineNode>),
    /// A group.
    Group(Box<GroupNode>),
    /// A live object: blend, contour, shadow, bevel, mould, brush or effect.
    /// Structure and round-trip only until Phase 13.
    Live(Box<LiveNode>),
    /// A clipping container.
    ClipView(ClipViewNode),
    /// A text story. Structure only until Phase 9.
    TextStory(Box<TextStoryNode>),
    /// One line of a text story.
    TextLine(Box<TextLineNode>),
    /// One character, kern, tab or line break.
    TextItem(TextItem),
    /// An attribute. Applies to its following siblings and their subtrees, and
    /// to its parent's own ink.
    Attr(Box<AttrNode>),
    /// Data from a newer producer we do not model, kept verbatim so that a
    /// round trip preserves it. Never rendered.
    Opaque(Box<OpaqueNode>),
}

impl NodeKind {
    /// Painted **after** its children.
    #[must_use]
    pub fn is_ink(&self) -> bool {
        !matches!(
            self,
            NodeKind::Document(_)
                | NodeKind::Chapter
                | NodeKind::Spread(_)
                | NodeKind::Page(_)
                | NodeKind::Layer(_)
                | NodeKind::Grid(_)
                | NodeKind::Attr(_)
                | NodeKind::Opaque(_)
        )
    }

    /// Painted **before** its children.
    #[must_use]
    pub fn is_paper(&self) -> bool {
        matches!(
            self,
            NodeKind::Document(_)
                | NodeKind::Chapter
                | NodeKind::Spread(_)
                | NodeKind::Page(_)
                | NodeKind::Layer(_)
                | NodeKind::Grid(_)
        )
    }

    /// Whether this is an attribute node.
    #[inline]
    #[must_use]
    pub fn is_attr(&self) -> bool {
        matches!(self, NodeKind::Attr(_))
    }

    /// Whether the node controls its children rather than merely containing
    /// them.
    #[must_use]
    pub fn is_compound(&self) -> bool {
        matches!(
            self,
            NodeKind::Group(_)
                | NodeKind::Live(_)
                | NodeKind::ClipView(_)
                | NodeKind::TextStory(_)
                | NodeKind::TextLine(_)
        )
    }

    /// Whether the node cannot exist outside its controller.
    #[must_use]
    pub fn needs_parent(&self) -> bool {
        match self {
            NodeKind::Live(l) => l.role == crate::live::LiveRole::Generated,
            NodeKind::TextLine(_) | NodeKind::TextItem(_) => true,
            _ => false,
        }
    }

    /// A short, stable name, used in dumps, diagnostics and errors.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Document(_) => "Document",
            NodeKind::Chapter => "Chapter",
            NodeKind::Spread(_) => "Spread",
            NodeKind::Page(_) => "Page",
            NodeKind::Layer(_) => "Layer",
            NodeKind::Grid(_) => "Grid",
            NodeKind::Path(_) => "Path",
            NodeKind::Shape(_) => "Shape",
            NodeKind::QuickShape(_) => "QuickShape",
            NodeKind::Bitmap(_) => "Bitmap",
            NodeKind::Guideline(_) => "Guideline",
            NodeKind::Group(_) => "Group",
            NodeKind::Live(_) => "Live",
            NodeKind::ClipView(_) => "ClipView",
            NodeKind::TextStory(_) => "TextStory",
            NodeKind::TextLine(_) => "TextLine",
            NodeKind::TextItem(_) => "TextItem",
            NodeKind::Attr(_) => "Attr",
            NodeKind::Opaque(_) => "Opaque",
        }
    }

    /// A stable discriminant, for the canonical digest and for dumps.
    #[must_use]
    pub fn discriminant(&self) -> u8 {
        match self {
            NodeKind::Document(_) => 0,
            NodeKind::Chapter => 1,
            NodeKind::Spread(_) => 2,
            NodeKind::Page(_) => 3,
            NodeKind::Layer(_) => 4,
            NodeKind::Grid(_) => 5,
            NodeKind::Path(_) => 6,
            NodeKind::Shape(_) => 7,
            NodeKind::QuickShape(_) => 8,
            NodeKind::Bitmap(_) => 9,
            NodeKind::Guideline(_) => 10,
            NodeKind::Group(_) => 11,
            NodeKind::Live(_) => 12,
            NodeKind::ClipView(_) => 13,
            NodeKind::TextStory(_) => 14,
            NodeKind::TextLine(_) => 15,
            NodeKind::TextItem(_) => 16,
            NodeKind::Attr(_) => 17,
            NodeKind::Opaque(_) => 18,
        }
    }

    /// An estimate of the bytes this payload retains, for the history budget
    /// and for the build limits. Shared `Arc` payloads are counted in full,
    /// which over-estimates on purpose: the budget is a safety limit.
    #[must_use]
    pub fn size_hint(&self) -> usize {
        let own = size_of::<NodeKind>();
        own + match self {
            NodeKind::Path(p) => p.data.verbs().len() + std::mem::size_of_val(p.data.points()),
            NodeKind::QuickShape(q) => q
                .path
                .as_ref()
                .map_or(0, |p| p.verbs().len() + std::mem::size_of_val(p.points())),
            NodeKind::Attr(a) => a.value.size_hint(),
            NodeKind::Opaque(o) => o.payload.len(),
            NodeKind::Document(_) => size_of::<DocumentNode>(),
            NodeKind::Spread(_) => size_of::<SpreadNode>(),
            NodeKind::Page(_) => size_of::<PageNode>(),
            NodeKind::Layer(_) => size_of::<LayerNode>(),
            NodeKind::Grid(_) => size_of::<GridNode>(),
            NodeKind::Live(_) => size_of::<LiveNode>(),
            NodeKind::TextStory(_) => size_of::<TextStoryNode>(),
            NodeKind::TextLine(_) => size_of::<TextLineNode>(),
            _ => 0,
        }
    }
}

/// A free-form path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathNode {
    /// The geometry. Copy-on-write: cloning the node does not copy the path.
    pub data: Arc<Path>,
    /// Whether the interior is painted.
    pub filled: bool,
    /// Whether the outline is painted.
    pub stroked: bool,
}

impl PathNode {
    /// A filled, stroked path.
    #[must_use]
    pub fn new(path: Path) -> PathNode {
        PathNode {
            data: Arc::new(path),
            filled: true,
            stroked: true,
        }
    }

    /// Mutable access, cloning the shared geometry only if it is shared.
    pub fn edit(&mut self) -> &mut Path {
        Arc::make_mut(&mut self.data)
    }
}

/// Whether a [`ShapeNode`] is a rectangle or an ellipse.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ShapeKind {
    /// A rectangle, possibly sheared and rotated.
    Rect,
    /// An ellipse inscribed in the parallelogram.
    Ellipse,
}

/// A rectangle or ellipse, held as a parallelogram exactly as the format does:
/// an origin and two edge vectors, so that rotation and shear are exact.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeNode {
    /// Rectangle or ellipse.
    pub shape: ShapeKind,
    /// The corner the two axes start from.
    pub origin: Point,
    /// The first edge vector.
    pub major: Vector,
    /// The second edge vector.
    pub minor: Vector,
}

/// A regular polygon or star, held as its parameters.
///
/// Turning the parameters into a path is Phase 7's job; the node only stores
/// them, plus an optional cached path the importer may supply.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickShape {
    /// Number of sides or points.
    pub sides: u32,
    /// Whether it is a star rather than a polygon.
    pub stellated: bool,
    /// Whether the edges are curved.
    pub curved: bool,
    /// Centre of the shape.
    pub centre: Point,
    /// The major axis.
    pub major: Vector,
    /// The minor axis.
    pub minor: Vector,
    /// How far the star's points are stellated, as a fraction.
    pub stellation_radius: f32,
    /// A path the importer already had. Regenerated when absent.
    pub path: Option<Arc<Path>>,
}

/// A placed bitmap: a parallelogram plus the resource it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct BitmapNode {
    /// The resource.
    pub image: BitmapId,
    /// The corner the two axes start from.
    pub origin: Point,
    /// The first edge vector.
    pub major: Vector,
    /// The second edge vector.
    pub minor: Vector,
}

/// A guideline on a guide layer.
#[derive(Clone, Debug, PartialEq)]
pub struct GuidelineNode {
    /// Horizontal, rather than vertical.
    pub horizontal: bool,
    /// Where it sits along the perpendicular axis.
    pub position: Mp,
    /// An override colour from the palette.
    pub colour: Option<xarast_color::ColourId>,
}

/// A group.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GroupNode {
    /// The name shown in the object gallery.
    pub name: Option<Arc<str>>,
    /// A "soft group": members stay selectable individually.
    pub soft: bool,
}

/// How a [`ClipViewNode`] clips.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum ClipViewMode {
    /// Keep what is inside the clipping path.
    #[default]
    Inside,
    /// Keep what is outside it.
    Outside,
}

/// A clipping container. The first child is the clipping path.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub struct ClipViewNode {
    /// Which side is kept.
    pub mode: ClipViewMode,
}

/// Data from a newer producer we do not model.
///
/// Kept verbatim so that a `.xarast` round trip preserves it. Never rendered,
/// never interpreted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueNode {
    /// The producer's own tag for the record.
    pub tag: u32,
    /// The bytes, exactly as they arrived.
    pub payload: Arc<[u8]>,
}

/// An arrowhead or arrow tail.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrowSpec {
    /// A named or predefined arrow, when the file used a reference.
    pub name: Option<Arc<str>>,
    /// The outline, when the file defined one.
    pub path: Option<Arc<Path>>,
    /// Width scale relative to the line width.
    pub width: f32,
    /// Height scale relative to the line width.
    pub height: f32,
}

/// Identification of a typeface. Glyph data is never embedded in `.xar`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypefaceRef {
    /// The full name, for example `"Arial Bold"`.
    pub full_name: Arc<str>,
    /// The family name.
    pub family: Arc<str>,
    /// The ten PANOSE bytes, when the file carried them.
    pub panose: Option<[u8; 10]>,
}

/// A named stroke shape: the "stroke type" attribute.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeDef {
    /// The stroke's name.
    pub name: Arc<str>,
    /// The nib outline, when one was defined.
    pub nib: Option<Arc<Path>>,
}

/// A variable-width profile along a stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct WidthProfile {
    /// Samples of the half-width, in order along the stroke.
    pub samples: Arc<[f32]>,
}

/// A reference to a brush definition.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BrushRef {
    /// The brush's name.
    pub name: Arc<str>,
}
