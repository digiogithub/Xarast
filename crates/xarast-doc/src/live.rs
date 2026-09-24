//! Live objects: blends, contours, shadows, bevels, moulds, brushes and
//! effects.
//!
//! The node exists, it validates, it digests and it survives a round trip.
//! What it *derives* is computed outside the tree, by [`crate::regen`]:
//! regeneration is never an edit, so it never appears in the undo log.
//!
//! The pattern the original spreads across a dozen class pairs is one shape:
//! a *controller* the user selects, a *source* subtree holding the untouched
//! originals, and a *generated* subtree holding the derived result.

use std::sync::Arc;

use xarast_geom::{BiasGain, Mp, Point};

use crate::kind::NodeKind;
use crate::text::Justification;
use crate::tree::{NodeId, Tree};

/// What part a node plays in a live object.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum LiveRole {
    /// The node the user sees and selects.
    Controller,
    /// A derived, recomputable result. Cannot exist outside its controller.
    Generated,
    /// The source data, untouched.
    Source,
}

impl LiveRole {
    /// Whether a node in this role cannot exist on its own: it is derived
    /// data, excluded from selection, from the clipboard and from
    /// independent deletion (the original's `NeedsParent`,
    /// `research/02 §6.1`).
    #[must_use]
    pub const fn needs_parent(self) -> bool {
        matches!(self, LiveRole::Generated)
    }
}

/// How stale a generated subtree is.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum RegenState {
    /// The derived result is current.
    #[default]
    Clean,
    /// It must be recomputed before the next use.
    Dirty,
    /// It must be recomputed at the next repaint.
    Deferred,
}

/// A live object node.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveNode {
    /// Controller, source or generated.
    pub role: LiveRole,
    /// Which kind of live object, and its parameters.
    pub kind: LiveKind,
    /// Regeneration state.
    pub regen: RegenState,
    /// The name shown in the object gallery.
    pub name: Option<Arc<str>>,
}

/// Which live object, and its parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum LiveKind {
    /// A blend between two or more objects.
    Blend(Box<BlendParams>),
    /// A contour inside or outside an object.
    Contour(Box<ContourParams>),
    /// A wall, floor or glow shadow.
    Shadow(Box<ShadowParams>),
    /// A bevel.
    Bevel(Box<BevelParams>),
    /// An envelope or perspective mould.
    Mould(Box<MouldParams>),
    /// A brush stroke.
    Brush(Box<BrushParams>),
    /// A plug-in effect.
    Effect(Box<EffectParams>),
}

impl LiveKind {
    /// A short, stable name for dumps and diagnostics.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            LiveKind::Blend(_) => "Blend",
            LiveKind::Contour(_) => "Contour",
            LiveKind::Shadow(_) => "Shadow",
            LiveKind::Bevel(_) => "Bevel",
            LiveKind::Mould(_) => "Mould",
            LiveKind::Brush(_) => "Brush",
            LiveKind::Effect(_) => "Effect",
        }
    }

    /// A stable discriminant for the canonical digest.
    #[must_use]
    pub fn discriminant(&self) -> u8 {
        match self {
            LiveKind::Blend(_) => 0,
            LiveKind::Contour(_) => 1,
            LiveKind::Shadow(_) => 2,
            LiveKind::Bevel(_) => 3,
            LiveKind::Mould(_) => 4,
            LiveKind::Brush(_) => 5,
            LiveKind::Effect(_) => 6,
        }
    }
}

/// Parameters of a blend.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendParams {
    /// Number of intermediate steps.
    pub steps: u32,
    /// Fixed spacing between steps, when the user entered one.
    pub step_distance: Option<Mp>,
    /// Whether nodes are matched one to one rather than by position.
    pub one_to_one: bool,
    /// Whether the steps are antialiased.
    pub antialias: bool,
    /// Whether the steps rotate with the path.
    pub tangential: bool,
    /// Whether the blend runs in reverse.
    pub reverse: bool,
    /// The spacing profile.
    pub profile: BiasGain,
}

/// Parameters of a contour.
#[derive(Clone, Debug, PartialEq)]
pub struct ContourParams {
    /// Number of steps.
    pub steps: u32,
    /// Contour width; the sign selects inside or outside.
    pub width: Mp,
    /// Whether the contour lies outside the object.
    pub outer: bool,
    /// Whether line widths count towards the offset.
    pub include_line_widths: bool,
    /// The corner treatment.
    pub join: xarast_geom::Join,
    /// The spacing profile.
    pub profile: BiasGain,
}

/// Which sort of shadow.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ShadowKind {
    /// Cast on a wall behind the object.
    Wall,
    /// Cast on a floor below the object.
    Floor,
    /// A glow around the object.
    Glow,
}

/// Parameters of a shadow.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowParams {
    /// Wall, floor or glow.
    pub kind: ShadowKind,
    /// Offset from the object.
    pub offset: Point,
    /// Blur radius.
    pub blur: Mp,
    /// Darkness, `0.0..=1.0`.
    pub darkness: f32,
    /// The blur profile.
    pub profile: BiasGain,
    /// Floor shadows only: the vertical scale.
    pub scale: f32,
    /// Floor shadows only: the tilt.
    pub tilt: f32,
}

/// Which sort of bevel.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum BevelType {
    /// A flat chamfer.
    #[default]
    Flat,
    /// A rounded edge.
    Round,
    /// A concave edge.
    Hollow,
}

/// Parameters of a bevel.
#[derive(Clone, Debug, PartialEq)]
pub struct BevelParams {
    /// The bevel's profile shape.
    pub bevel_type: BevelType,
    /// How far the bevel reaches in from the edge.
    pub indent: Mp,
    /// Whether the bevel is outside the object.
    pub outer: bool,
    /// Direction of the light, in radians.
    pub light_angle: f32,
    /// Elevation of the light, in radians.
    pub light_tilt: f32,
    /// Contrast of the lighting.
    pub contrast: f32,
}

/// Which sort of mould.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum MouldKind {
    /// A four-sided envelope.
    Envelope,
    /// A perspective quadrilateral.
    Perspective,
}

/// Parameters of a mould. The cage itself is a child path node.
#[derive(Clone, Debug, PartialEq)]
pub struct MouldParams {
    /// Envelope or perspective.
    pub kind: MouldKind,
    /// The source rectangle the cage maps from.
    pub source: xarast_geom::Rect,
}

/// Parameters of a brush stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushParams {
    /// The brush definition's name.
    pub brush: Arc<str>,
    /// Spacing between stamps.
    pub spacing: Mp,
    /// Scale applied to each stamp.
    pub scale: f32,
}

/// Parameters of a plug-in effect, kept opaque until Phase 13.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectParams {
    /// The effect's identifier.
    pub id: Arc<str>,
    /// The effect's serialised settings, kept verbatim.
    pub settings: Arc<[u8]>,
    /// Whether the effect is locked to the object.
    pub locked: bool,
}

/// Text justification is shared with the text model; re-exported here so the
/// live-object parameters that need it do not reach across modules.
pub type LiveJustification = Justification;

/// The parts of a live controller.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LiveParts {
    /// The controller itself.
    pub controller: NodeId,
    /// Its source subtree: the originals, untouched.
    pub source: NodeId,
    /// Its first generated child, when a file stored one (the baked result
    /// a reader without a generator draws).
    pub generated: Option<NodeId>,
}

/// The source and generated children of a controller, or `None` when `id`
/// is not a controller or has no source (which `validate` reports).
#[must_use]
pub fn parts(tree: &Tree, id: NodeId) -> Option<LiveParts> {
    match tree.kind(id) {
        Some(NodeKind::Live(l)) if l.role == LiveRole::Controller => {}
        _ => return None,
    }
    let mut source = None;
    let mut generated = None;
    for c in tree.children(id) {
        if let Some(NodeKind::Live(l)) = tree.kind(c) {
            match l.role {
                LiveRole::Source if source.is_none() => source = Some(c),
                LiveRole::Generated if generated.is_none() => generated = Some(c),
                _ => {}
            }
        }
    }
    Some(LiveParts {
        controller: id,
        source: source?,
        generated,
    })
}

/// The nearest live controller at or above `id`.
#[must_use]
pub fn controller_of(tree: &Tree, id: NodeId) -> Option<NodeId> {
    std::iter::once(id).chain(tree.ancestors(id)).find(
        |a| matches!(tree.kind(*a), Some(NodeKind::Live(l)) if l.role == LiveRole::Controller),
    )
}

/// Whether `id` is a generated node or lies inside one: derived data that
/// no command may edit on its own ([`LiveRole::needs_parent`]).
#[must_use]
pub fn in_generated(tree: &Tree, id: NodeId) -> bool {
    std::iter::once(id)
        .chain(tree.ancestors(id))
        .any(|a| matches!(tree.kind(a), Some(NodeKind::Live(l)) if l.role.needs_parent()))
}
