//! Snapping (`phase-07 §W8`, XARA-US-0036): the grid, the guides and —
//! through the same trait — other objects.
//!
//! Snapping belongs to the *gesture*, not to the tool (`phase-07 §W8`,
//! "tricky part"): a tool hands the candidate document point to
//! [`ToolCtx::snap_point`](crate::tool::ToolCtx::snap_point) (or a moved
//! box to [`ToolCtx::snap_move`](crate::tool::ToolCtx::snap_move)) and
//! gets back the snapped point plus what it snapped to, for the feedback
//! marker. Every source is a [`SnapSource`]; adding object snapping is one
//! more implementation and nothing else changes.
//!
//! # The rules
//!
//! * The radius is in **device pixels** ([`SnapSettings::radius_px`]), so a
//!   snap feels the same at every zoom (`phase-07` K9).
//! * Each axis snaps on its own: a vertical guide pulls `x` only, a grid
//!   pulls `x` to the nearest vertical line and `y` to the nearest
//!   horizontal one. Both inside the radius is an intersection, landed on
//!   exactly (integer millipoints, no rounding drift).
//! * Per axis the nearest candidate wins; on a tie the higher
//!   [`SnapKind::priority`] (guides over objects over the grid).
//! * The switches are session state ([`EditState::snap`](crate::EditState)),
//!   toggled mid-drag by the keypad (`NumPad .` grid, `NumPad 2` guides,
//!   `NumPad *` objects, `research/04 §4.4`); a toggle re-evaluates the
//!   drag at once.

use xarast_doc::{Document, GridKind, NodeId, NodeKind};
use xarast_geom::{Mp, Vector};

use crate::geometry::{DocPoint, DocRect};
use crate::viewport::Viewport;

/// What a point can snap to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapKind {
    /// The rectangular grid of the spread.
    Grid,
    /// The guidelines.
    Guide,
    /// Other objects' points and outlines.
    Object,
}

impl SnapKind {
    /// Tie-break order: larger wins.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            SnapKind::Grid => 0,
            SnapKind::Object => 1,
            SnapKind::Guide => 2,
        }
    }
}

/// Which coordinates a candidate fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapAxis {
    /// `x` only: a vertical line.
    X,
    /// `y` only: a horizontal line.
    Y,
    /// Both: a point.
    Both,
}

/// One place a point could snap to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapCandidate {
    /// The snapped position. For [`SnapAxis::X`] only `x` means anything,
    /// for [`SnapAxis::Y`] only `y`.
    pub at: DocPoint,
    /// Which coordinates it fixes.
    pub axis: SnapAxis,
    /// Where it came from.
    pub kind: SnapKind,
}

/// What a snap landed on, for the feedback marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapHit {
    /// The kind of the winning candidate (the `x` one if the axes differ).
    pub kind: SnapKind,
    /// The snapped point.
    pub at: DocPoint,
    /// Which coordinates were snapped.
    pub axis: SnapAxis,
}

/// Something a point can snap to.
pub trait SnapSource {
    /// Its kind.
    fn kind(&self) -> SnapKind;
    /// Every candidate within `radius` millipoints of `near` (per axis for
    /// line candidates).
    fn candidates(&self, near: DocPoint, radius: Mp, out: &mut Vec<SnapCandidate>);
}

/// The snapping switches and radius: session state, never undone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapSettings {
    /// Snap to the grid.
    pub grid: bool,
    /// Snap to guides.
    pub guides: bool,
    /// Snap to objects.
    pub objects: bool,
    /// How close a candidate must be, in device pixels. Provisional
    /// (8 px); the original's default radius is a preference to observe.
    pub radius_px: f64,
}

impl Default for SnapSettings {
    fn default() -> SnapSettings {
        SnapSettings {
            grid: false,
            guides: true,
            objects: false,
            radius_px: SNAP_RADIUS_PX,
        }
    }
}

/// The default snap radius, in device pixels.
pub const SNAP_RADIUS_PX: f64 = 8.0;

impl SnapSettings {
    /// Whether a kind is switched on.
    #[must_use]
    pub const fn enabled(&self, kind: SnapKind) -> bool {
        match kind {
            SnapKind::Grid => self.grid,
            SnapKind::Guide => self.guides,
            SnapKind::Object => self.objects,
        }
    }

    /// Switches a kind on or off.
    pub fn set_enabled(&mut self, kind: SnapKind, on: bool) {
        match kind {
            SnapKind::Grid => self.grid = on,
            SnapKind::Guide => self.guides = on,
            SnapKind::Object => self.objects = on,
        }
    }

    /// Whether anything is switched on.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.grid || self.guides || self.objects
    }
}

/// A rectangular grid: lines every `step` from `origin` on both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSource {
    /// A grid intersection.
    pub origin: DocPoint,
    /// The distance between two snapping lines (the minor spacing).
    pub step: Mp,
}

impl GridSource {
    /// The snapping grid of the active spread: its first rectangular
    /// [`GridNode`](xarast_doc::GridNode), at its minor spacing.
    #[must_use]
    pub fn of(doc: &Document) -> Option<GridSource> {
        let spread = doc.active_spread();
        doc.tree
            .children(spread)
            .find_map(|c| match doc.tree.kind(c) {
                Some(NodeKind::Grid(g)) if g.kind == GridKind::Rect => {
                    let subs = i32::try_from(g.subdivisions.max(1)).unwrap_or(1);
                    let step = Mp::new((g.spacing.raw() / subs).max(1));
                    Some(GridSource {
                        origin: g.origin,
                        step,
                    })
                }
                _ => None,
            })
    }

    /// The nearest grid coordinate to `v` along an axis whose lines pass
    /// through `o`. Exact integer arithmetic; ties round up.
    #[must_use]
    pub fn nearest(o: Mp, step: Mp, v: Mp) -> Mp {
        let (o, s, v) = (
            i64::from(o.raw()),
            i64::from(step.raw()).max(1),
            i64::from(v.raw()),
        );
        let k = (v - o + s / 2).div_euclid(s);
        let r = o + k * s;
        Mp::new(i32::try_from(r.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0))
    }
}

impl SnapSource for GridSource {
    fn kind(&self) -> SnapKind {
        SnapKind::Grid
    }

    fn candidates(&self, near: DocPoint, radius: Mp, out: &mut Vec<SnapCandidate>) {
        let x = GridSource::nearest(self.origin.x, self.step, near.x);
        let y = GridSource::nearest(self.origin.y, self.step, near.y);
        if (i64::from(x.raw()) - i64::from(near.x.raw())).abs() <= i64::from(radius.raw()) {
            out.push(SnapCandidate {
                at: DocPoint::new(x, near.y),
                axis: SnapAxis::X,
                kind: SnapKind::Grid,
            });
        }
        if (i64::from(y.raw()) - i64::from(near.y.raw())).abs() <= i64::from(radius.raw()) {
            out.push(SnapCandidate {
                at: DocPoint::new(near.x, y),
                axis: SnapAxis::Y,
                kind: SnapKind::Grid,
            });
        }
    }
}

/// One guideline, as snapping sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuideLine {
    /// Horizontal: fixes `y`. Vertical: fixes `x`.
    pub horizontal: bool,
    /// Where along the perpendicular axis.
    pub position: Mp,
    /// The guideline node.
    pub node: NodeId,
}

/// The guidelines of the active spread's visible guide layers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuideSource {
    /// The guides.
    pub guides: Vec<GuideLine>,
}

/// Every guideline of the active spread, visible or not, in document
/// order.
#[must_use]
pub fn guidelines(doc: &Document) -> Vec<GuideLine> {
    let spread = doc.active_spread();
    let mut out = Vec::new();
    for layer in doc.tree.children(spread) {
        if !matches!(doc.tree.kind(layer), Some(NodeKind::Layer(l)) if l.guide) {
            continue;
        }
        for g in doc.tree.children(layer) {
            if let Some(NodeKind::Guideline(gl)) = doc.tree.kind(g) {
                out.push(GuideLine {
                    horizontal: gl.horizontal,
                    position: gl.position,
                    node: g,
                });
            }
        }
    }
    out
}

/// The active spread's guide layer, if it has one.
#[must_use]
pub fn guide_layer(doc: &Document) -> Option<NodeId> {
    let spread = doc.active_spread();
    doc.tree
        .children(spread)
        .find(|l| matches!(doc.tree.kind(*l), Some(NodeKind::Layer(x)) if x.guide))
}

/// Whether the guides are shown (the guide layer is visible).
#[must_use]
pub fn guides_visible(doc: &Document) -> bool {
    guide_layer(doc)
        .is_none_or(|l| matches!(doc.tree.kind(l), Some(NodeKind::Layer(x)) if x.visible))
}

impl GuideSource {
    /// The guides snapping uses: those of a visible guide layer.
    #[must_use]
    pub fn of(doc: &Document) -> GuideSource {
        if !guides_visible(doc) {
            return GuideSource::default();
        }
        GuideSource {
            guides: guidelines(doc),
        }
    }
}

impl SnapSource for GuideSource {
    fn kind(&self) -> SnapKind {
        SnapKind::Guide
    }

    fn candidates(&self, near: DocPoint, radius: Mp, out: &mut Vec<SnapCandidate>) {
        let r = i64::from(radius.raw());
        for g in &self.guides {
            let (v, axis, at) = if g.horizontal {
                (near.y, SnapAxis::Y, DocPoint::new(near.x, g.position))
            } else {
                (near.x, SnapAxis::X, DocPoint::new(g.position, near.y))
            };
            if (i64::from(g.position.raw()) - i64::from(v.raw())).abs() <= r {
                out.push(SnapCandidate {
                    at,
                    axis,
                    kind: SnapKind::Guide,
                });
            }
        }
    }
}

/// The resolver: picks, per axis, the nearest candidate of the enabled
/// sources.
pub struct SnapResolver<'a> {
    sources: Vec<Box<dyn SnapSource + 'a>>,
    radius: Mp,
}

impl std::fmt::Debug for SnapResolver<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapResolver")
            .field("sources", &self.sources.len())
            .field("radius", &self.radius)
            .finish()
    }
}

impl<'a> SnapResolver<'a> {
    /// A resolver over explicit sources, with a radius in millipoints.
    #[must_use]
    pub fn new(sources: Vec<Box<dyn SnapSource + 'a>>, radius: Mp) -> SnapResolver<'a> {
        SnapResolver { sources, radius }
    }

    /// The resolver a document, its snapping switches and a view give:
    /// the radius converted from device pixels at the view's zoom.
    #[must_use]
    pub fn for_document(
        doc: &'a Document,
        settings: &SnapSettings,
        vp: &Viewport,
        exclude: &'a [NodeId],
    ) -> SnapResolver<'a> {
        let mut sources: Vec<Box<dyn SnapSource + 'a>> = Vec::new();
        if settings.guides {
            let g = GuideSource::of(doc);
            if !g.guides.is_empty() {
                sources.push(Box::new(g));
            }
        }
        if settings.grid
            && let Some(g) = GridSource::of(doc)
        {
            sources.push(Box::new(g));
        }
        if settings.objects {
            sources.push(Box::new(ObjectSource::new(doc, exclude)));
        }
        let s = vp.scale();
        let per_px = if s > 0.0 && s.is_finite() {
            1.0 / s
        } else {
            1.0
        };
        SnapResolver::new(sources, Mp::from_f64_round(settings.radius_px * per_px))
    }

    /// Whether any source is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// The radius, in millipoints.
    #[must_use]
    pub const fn radius(&self) -> Mp {
        self.radius
    }

    /// Snaps a point. Returns it unchanged, and no hit, when nothing is
    /// within the radius.
    #[must_use]
    pub fn snap(&self, p: DocPoint) -> (DocPoint, Option<SnapHit>) {
        let mut cands = Vec::new();
        for s in &self.sources {
            s.candidates(p, self.radius, &mut cands);
        }
        let dist = |a: Mp, b: Mp| (i64::from(a.raw()) - i64::from(b.raw())).abs();
        // A point candidate competes on both axes at once with its
        // distance; line candidates on their own axis.
        let mut best_x: Option<(i64, u8, SnapCandidate)> = None;
        let mut best_y: Option<(i64, u8, SnapCandidate)> = None;
        let mut best_pt: Option<(i64, u8, SnapCandidate)> = None;
        let better = |cur: &Option<(i64, u8, SnapCandidate)>, d: i64, pr: u8| {
            cur.is_none_or(|(cd, cp, _)| d < cd || (d == cd && pr > cp))
        };
        for c in cands {
            let pr = c.kind.priority();
            match c.axis {
                SnapAxis::X => {
                    let d = dist(c.at.x, p.x);
                    if better(&best_x, d, pr) {
                        best_x = Some((d, pr, c));
                    }
                }
                SnapAxis::Y => {
                    let d = dist(c.at.y, p.y);
                    if better(&best_y, d, pr) {
                        best_y = Some((d, pr, c));
                    }
                }
                SnapAxis::Both => {
                    let d = (c.at.x.to_f64() - p.x.to_f64())
                        .hypot(c.at.y.to_f64() - p.y.to_f64())
                        .round() as i64;
                    if d <= i64::from(self.radius.raw()) && better(&best_pt, d, pr) {
                        best_pt = Some((d, pr, c));
                    }
                }
            }
        }
        // A point wins over the per-axis pair when it is at least as close
        // as the closer of the two.
        if let Some((d, pr, c)) = best_pt {
            let axis_best = [best_x, best_y]
                .iter()
                .flatten()
                .map(|(d, p, _)| (*d, *p))
                .min_by_key(|(d, p)| (*d, std::cmp::Reverse(*p)));
            if axis_best.is_none_or(|(ad, ap)| d < ad || (d == ad && pr >= ap)) {
                return (
                    c.at,
                    Some(SnapHit {
                        kind: c.kind,
                        at: c.at,
                        axis: SnapAxis::Both,
                    }),
                );
            }
        }
        let x = best_x.map(|(_, _, c)| c);
        let y = best_y.map(|(_, _, c)| c);
        let at = DocPoint::new(x.map_or(p.x, |c| c.at.x), y.map_or(p.y, |c| c.at.y));
        let hit = match (x, y) {
            (None, None) => None,
            (Some(c), None) => Some(SnapHit {
                kind: c.kind,
                at,
                axis: SnapAxis::X,
            }),
            (None, Some(c)) => Some(SnapHit {
                kind: c.kind,
                at,
                axis: SnapAxis::Y,
            }),
            (Some(c), Some(_)) => Some(SnapHit {
                kind: c.kind,
                at,
                axis: SnapAxis::Both,
            }),
        };
        (at, hit)
    }

    /// Snaps a box being moved by `delta`: tries its nine anchor points
    /// and takes, per axis, the smallest correction any of them needs.
    /// Returns the corrected displacement.
    #[must_use]
    pub fn snap_move(&self, bounds: DocRect, delta: Vector) -> (Vector, Option<SnapHit>) {
        if self.sources.is_empty() || bounds.is_empty() {
            return (delta, None);
        }
        let moved = DocRect {
            lo: bounds.lo + delta,
            hi: bounds.hi + delta,
        };
        let xs = [moved.lo.x, mid(moved.lo.x, moved.hi.x), moved.hi.x];
        let ys = [moved.lo.y, mid(moved.lo.y, moved.hi.y), moved.hi.y];
        let mut best_x: Option<(i64, i64, SnapHit)> = None;
        let mut best_y: Option<(i64, i64, SnapHit)> = None;
        for x in xs {
            for y in ys {
                let p = DocPoint::new(x, y);
                let (q, hit) = self.snap(p);
                let Some(hit) = hit else { continue };
                let (cx, cy) = (
                    i64::from(q.x.raw()) - i64::from(p.x.raw()),
                    i64::from(q.y.raw()) - i64::from(p.y.raw()),
                );
                if matches!(hit.axis, SnapAxis::X | SnapAxis::Both)
                    && best_x.is_none_or(|(b, _, _)| cx.abs() < b.abs())
                {
                    best_x = Some((cx, cx, hit));
                }
                if matches!(hit.axis, SnapAxis::Y | SnapAxis::Both)
                    && best_y.is_none_or(|(b, _, _)| cy.abs() < b.abs())
                {
                    best_y = Some((cy, cy, hit));
                }
            }
        }
        let fix = |v: Mp, c: Option<(i64, i64, SnapHit)>| {
            c.map_or(v, |(d, _, _)| {
                Mp::new(v.raw().saturating_add(i32::try_from(d).unwrap_or(0)))
            })
        };
        let out = Vector::new(fix(delta.dx, best_x), fix(delta.dy, best_y));
        let hit = best_x.or(best_y).map(|(_, _, h)| h);
        (out, hit)
    }
}

fn mid(a: Mp, b: Mp) -> Mp {
    Mp::new(((i64::from(a.raw()) + i64::from(b.raw())).div_euclid(2)) as i32)
}

/// Snapping to other objects: their bounding-box corners and centres
/// (`phase-07` reserves the full magnetic snap to outlines, XARA-T-0153).
#[derive(Debug, Clone, Default)]
pub struct ObjectSource {
    points: Vec<DocPoint>,
}

impl ObjectSource {
    /// The snap points of every selectable object except `exclude` (the
    /// objects being dragged).
    #[must_use]
    pub fn new(doc: &Document, exclude: &[NodeId]) -> ObjectSource {
        let mut points = Vec::new();
        for n in crate::edit::selectable_objects(doc) {
            if exclude.contains(&n) {
                continue;
            }
            let b = crate::viewport::nodes_rect(doc, [n]);
            if b.is_empty() {
                continue;
            }
            for x in [b.lo.x, mid(b.lo.x, b.hi.x), b.hi.x] {
                for y in [b.lo.y, mid(b.lo.y, b.hi.y), b.hi.y] {
                    points.push(DocPoint::new(x, y));
                }
            }
        }
        ObjectSource { points }
    }
}

impl SnapSource for ObjectSource {
    fn kind(&self) -> SnapKind {
        SnapKind::Object
    }

    fn candidates(&self, near: DocPoint, radius: Mp, out: &mut Vec<SnapCandidate>) {
        let r = radius.to_f64();
        for p in &self.points {
            if (p.x.to_f64() - near.x.to_f64()).hypot(p.y.to_f64() - near.y.to_f64()) <= r {
                out.push(SnapCandidate {
                    at: *p,
                    axis: SnapAxis::Both,
                    kind: SnapKind::Object,
                });
            }
        }
    }
}

// ── guide and grid commands ─────────────────────────────────────────────

/// What a guide or grid command changes.
#[derive(Debug, Clone, PartialEq)]
pub enum GuideOp {
    /// Adds a guideline to the active spread's guide layer (creating the
    /// layer when the spread has none).
    Add {
        /// Horizontal (fixes `y`) or vertical (fixes `x`).
        horizontal: bool,
        /// Where along the perpendicular axis.
        position: Mp,
    },
    /// Moves a guideline.
    Move {
        /// The guideline node.
        guide: NodeId,
        /// Its new position.
        position: Mp,
    },
    /// Deletes a guideline.
    Delete(NodeId),
    /// Deletes every guideline of the active spread.
    DeleteAll,
    /// Replaces the active spread's grid (creating one when it has none).
    SetGrid(xarast_doc::GridNode),
}

/// A guide or grid edit, as the bus runs it. Guides and the grid are
/// document state: they are saved in `.xarast` (`xarast:guideline`,
/// `xarast:grid`) and undone like everything else.
#[derive(Debug, Clone, PartialEq)]
pub struct GuideCommand(pub GuideOp);

impl xarast_doc::Command for GuideCommand {
    fn label(&self) -> &'static str {
        match self.0 {
            GuideOp::Add { .. } => "Add Guide",
            GuideOp::Move { .. } => "Move Guide",
            GuideOp::Delete(_) => "Delete Guide",
            GuideOp::DeleteAll => "Delete All Guides",
            GuideOp::SetGrid(_) => "Grid Settings",
        }
    }

    fn run(&self, tx: &mut xarast_doc::Tx<'_>) -> Result<(), xarast_doc::EditError> {
        use xarast_doc::{Attach, EditError, GuidelineNode, LayerNode};
        match &self.0 {
            GuideOp::Add {
                horizontal,
                position,
            } => {
                let layer = if let Some(l) = guide_layer(tx.doc()) {
                    l
                } else {
                    let spread = tx.doc().active_spread();
                    let l = tx.create(NodeKind::Layer(Box::new(LayerNode {
                        name: std::sync::Arc::from("Guides"),
                        guide: true,
                        active: false,
                        printable: false,
                        ..LayerNode::default()
                    })))?;
                    tx.attach(l, spread, Attach::LastChild)?;
                    l
                };
                let g = tx.create(NodeKind::Guideline(Box::new(GuidelineNode {
                    horizontal: *horizontal,
                    position: *position,
                    colour: None,
                })))?;
                tx.attach(g, layer, Attach::LastChild)
            }
            GuideOp::Move { guide, position } => {
                let Some(NodeKind::Guideline(g)) = tx.doc().tree.kind(*guide) else {
                    return Err(EditError::WrongKind(*guide));
                };
                let mut g = (**g).clone();
                if g.position == *position {
                    return Ok(());
                }
                g.position = *position;
                tx.set_kind(*guide, NodeKind::Guideline(Box::new(g)))
            }
            GuideOp::Delete(guide) => {
                if !matches!(tx.doc().tree.kind(*guide), Some(NodeKind::Guideline(_))) {
                    return Err(EditError::WrongKind(*guide));
                }
                tx.delete(*guide)
            }
            GuideOp::DeleteAll => {
                for g in guidelines(tx.doc()) {
                    tx.delete(g.node)?;
                }
                Ok(())
            }
            GuideOp::SetGrid(grid) => {
                let spread = tx.doc().active_spread();
                let existing = tx
                    .doc()
                    .tree
                    .children(spread)
                    .find(|c| matches!(tx.doc().tree.kind(*c), Some(NodeKind::Grid(_))));
                match existing {
                    Some(n) => {
                        if matches!(tx.doc().tree.kind(n), Some(NodeKind::Grid(g)) if **g == *grid)
                        {
                            return Ok(());
                        }
                        tx.set_kind(n, NodeKind::Grid(Box::new(grid.clone())))
                    }
                    None => {
                        let n = tx.create(NodeKind::Grid(Box::new(grid.clone())))?;
                        tx.attach(n, spread, Attach::FirstChild)
                    }
                }
            }
        }
    }
}

/// The active spread's grid, or the default one when it has none.
#[must_use]
pub fn grid_of(doc: &Document) -> xarast_doc::GridNode {
    let spread = doc.active_spread();
    doc.tree
        .children(spread)
        .find_map(|c| match doc.tree.kind(c) {
            Some(NodeKind::Grid(g)) => Some((**g).clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(step: i32) -> SnapResolver<'static> {
        SnapResolver::new(
            vec![Box::new(GridSource {
                origin: DocPoint::raw(0, 0),
                step: Mp::new(step),
            })],
            Mp::new(800),
        )
    }

    #[test]
    fn a_grid_snap_lands_exactly_on_an_intersection() {
        let r = grid(9_000);
        let (p, hit) = r.snap(DocPoint::raw(17_700, -8_600));
        assert_eq!(p, DocPoint::raw(18_000, -9_000));
        assert_eq!(hit.map(|h| h.axis), Some(SnapAxis::Both));
        // Outside the radius on x, inside on y.
        let (p, hit) = r.snap(DocPoint::raw(13_000, 9_500));
        assert_eq!(p, DocPoint::raw(13_000, 9_000));
        assert_eq!(hit.map(|h| h.axis), Some(SnapAxis::Y));
        // Outside on both.
        assert_eq!(r.snap(DocPoint::raw(4_500, 4_500)).1, None);
    }

    #[test]
    fn nearest_grid_line_is_exact_for_negative_and_offset_origins() {
        assert_eq!(
            GridSource::nearest(Mp::new(500), Mp::new(1_000), Mp::new(-1_100)),
            Mp::new(-1_500)
        );
        assert_eq!(
            GridSource::nearest(Mp::new(0), Mp::new(1_000), Mp::new(1_500)),
            Mp::new(2_000)
        );
        assert_eq!(
            GridSource::nearest(Mp::new(0), Mp::new(1_000), Mp::new(-1_500)),
            Mp::new(-1_000)
        );
    }

    #[test]
    fn a_guide_beats_the_grid_on_a_tie_and_the_nearer_wins_otherwise() {
        let guides = GuideSource {
            guides: vec![GuideLine {
                horizontal: false,
                position: Mp::new(10_300),
                node: NodeId::default(),
            }],
        };
        let r = SnapResolver::new(
            vec![
                Box::new(GridSource {
                    origin: DocPoint::raw(0, 0),
                    step: Mp::new(10_000),
                }),
                Box::new(guides),
            ],
            Mp::new(800),
        );
        let (p, hit) = r.snap(DocPoint::raw(10_200, 50_000));
        assert_eq!(p.x, Mp::new(10_300), "the guide is 100 away, the grid 200");
        assert_eq!(hit.unwrap().kind, SnapKind::Guide);
        let (p, _) = r.snap(DocPoint::raw(10_050, 50_000));
        assert_eq!(p.x, Mp::new(10_000), "the grid is nearer now");
        let (p, hit) = r.snap(DocPoint::raw(10_150, 50_000));
        assert_eq!(
            (p.x, hit.unwrap().kind),
            (Mp::new(10_300), SnapKind::Guide),
            "tie → guide"
        );
    }

    #[test]
    fn a_moved_box_snaps_its_nearest_edge() {
        let r = grid(10_000);
        let b = DocRect {
            lo: DocPoint::raw(1_000, 1_000),
            hi: DocPoint::raw(5_000, 3_000),
        };
        // Moving by (8 700, 0): the left edge would sit at 9 700 (300 from
        // 10 000); the right at 13 700; the centre at 11 700.
        let (d, hit) = r.snap_move(b, Vector::raw(8_700, 0));
        assert_eq!(d.dx, Mp::new(9_000));
        assert!(hit.is_some());
    }
}
