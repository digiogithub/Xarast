//! Node-level path editing: the geometry behind the shape editor and the
//! pen tool.
//!
//! A [`Path`] is segment-oriented (one verb per segment, three points per
//! cubic), which is what renderers want and what an editor does not: the
//! editor thinks in *nodes* — on-curve endpoints, each with an optional
//! incoming and outgoing control handle. [`EditPath`] is that view. It is
//! built from a path, edited, and written back; the round trip
//! `EditPath::from_path(p).to_path() == p` is exact for every well-formed
//! path whose subpaths have at least two nodes.
//!
//! # The node model
//!
//! Segment `i` of a subpath runs from node `i` to node `i + 1` (and, in a
//! closed subpath, the last segment runs from the last node back to node
//! 0). It is a cubic exactly when node `i` has an outgoing handle, and then
//! node `i + 1` has an incoming one: the two are created and removed
//! together.
//!
//! A closed `.xar` subpath usually repeats its first point as its last one
//! and then sets the close bit; a closed SVG subpath usually does not. Both
//! are one node here — the repeated point is folded into node 0 — and the
//! subpath remembers which way it was written, so that writing it back
//! produces the same points.
//!
//! # Point indices
//!
//! The session keeps the selected nodes as indices into
//! [`Path::points`] (`document-model.md` decision 9: selection is not in
//! the geometry). [`EditPath::node_point_index`] and
//! [`EditPath::node_at_point_index`] translate between the two views.
//!
//! # The flag model (`research/04 §4.11`)
//!
//! Two point flags drive editing, exactly as the `.xar` flags mean them:
//!
//! * `ROTATE` on a node makes it **smooth**: its two handles are kept
//!   collinear, and dragging one turns the other (keeping that one's
//!   length). A node without it is a cusp.
//! * `SMOOTH` on a *handle* means the handle is **auto-placed**: moving a
//!   neighbouring node recomputes it ([`EditPath::resmooth_around`]).
//!   Dragging a handle by hand clears it.
//!
//! # Continuity
//!
//! [`EditPath::split`] cuts a cubic with de Casteljau's construction, so
//! the curve does not move and the new node is smooth. [`EditPath::delete`]
//! is its inverse: it rebuilds one cubic from the two either side of the
//! deleted node, keeping the outer handles' directions (so the neighbours
//! keep their continuity) and recovering the split parameter from the
//! ratio of the deleted node's handle lengths. Deleting a node that
//! [`EditPath::split`] added gives back the original curve to within the
//! millipoint rounding of the split.

use kurbo::{ParamCurve, ParamCurveArclen, ParamCurveNearest};

use crate::{Path, Point, PointFlags, Segment, Vector, Verb};

/// A control handle: a Bézier control point and its per-point flags.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Control {
    /// Where it is.
    pub at: Point,
    /// Its `.xar` point flags, preserved across edits.
    pub flags: PointFlags,
}

impl Control {
    /// A control point with no flags.
    #[must_use]
    pub const fn at(p: Point) -> Control {
        Control {
            at: p,
            flags: PointFlags::empty(),
        }
    }
}

/// One on-curve node and its handles.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditNode {
    /// Where the curve passes.
    pub at: Point,
    /// The endpoint's point flags.
    pub flags: PointFlags,
    /// The handle of the segment arriving here, when that segment is a
    /// cubic.
    pub ctrl_in: Option<Control>,
    /// The handle of the segment leaving here, when that segment is a
    /// cubic.
    pub ctrl_out: Option<Control>,
}

impl EditNode {
    /// A corner node with no handles.
    #[must_use]
    pub const fn corner(at: Point) -> EditNode {
        EditNode {
            at,
            flags: PointFlags::END_POINT,
            ctrl_in: None,
            ctrl_out: None,
        }
    }

    /// Whether the node is smooth: its handles are kept collinear (the
    /// `ROTATE` flag).
    #[must_use]
    pub const fn is_smooth(&self) -> bool {
        self.flags.contains(PointFlags::ROTATE)
    }

    fn translate(&mut self, by: Vector) {
        self.at += by;
        if let Some(c) = &mut self.ctrl_in {
            c.at += by;
        }
        if let Some(c) = &mut self.ctrl_out {
            c.at += by;
        }
    }

    /// The handle on one side.
    #[must_use]
    pub const fn control(&self, side: Side) -> Option<Control> {
        match side {
            Side::In => self.ctrl_in,
            Side::Out => self.ctrl_out,
        }
    }

    fn control_mut(&mut self, side: Side) -> &mut Option<Control> {
        match side {
            Side::In => &mut self.ctrl_in,
            Side::Out => &mut self.ctrl_out,
        }
    }
}

/// Which handle of a node.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Side {
    /// The handle of the segment arriving at the node.
    In,
    /// The handle of the segment leaving it.
    Out,
}

impl Side {
    /// The other handle.
    #[must_use]
    pub const fn opposite(self) -> Side {
        match self {
            Side::In => Side::Out,
            Side::Out => Side::In,
        }
    }
}

/// One subpath as a run of nodes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditSubpath {
    /// The nodes, in path order.
    pub nodes: Vec<EditNode>,
    /// Whether the last node joins the first.
    pub closed: bool,
    /// Whether the closing segment is written out explicitly, ending on a
    /// repeat of the first point (the `.xar` way), rather than left to the
    /// `Close` verb.
    explicit_close: bool,
    /// The flags of that repeated point.
    close_flags: PointFlags,
}

impl EditSubpath {
    /// An open subpath through `nodes`.
    #[must_use]
    pub fn open(nodes: Vec<EditNode>) -> EditSubpath {
        EditSubpath {
            nodes,
            closed: false,
            explicit_close: false,
            close_flags: PointFlags::empty(),
        }
    }

    /// How many segments it has.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        let n = self.nodes.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }

    /// The node after `i`, wrapping in a closed subpath.
    #[must_use]
    pub fn next(&self, i: usize) -> Option<usize> {
        let n = self.nodes.len();
        if i + 1 < n {
            Some(i + 1)
        } else if self.closed && n > 1 {
            Some(0)
        } else {
            None
        }
    }

    /// The node before `i`, wrapping in a closed subpath.
    #[must_use]
    pub fn prev(&self, i: usize) -> Option<usize> {
        let n = self.nodes.len();
        if i > 0 {
            Some(i - 1)
        } else if self.closed && n > 1 {
            Some(n - 1)
        } else {
            None
        }
    }
}

/// A node, by subpath and position.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeRef {
    /// The subpath.
    pub subpath: usize,
    /// The node within it.
    pub node: usize,
}

impl NodeRef {
    /// Node `node` of subpath `subpath`.
    #[must_use]
    pub const fn new(subpath: usize, node: usize) -> NodeRef {
        NodeRef { subpath, node }
    }
}

/// A segment, by subpath and position: segment `i` leaves node `i`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SegRef {
    /// The subpath.
    pub subpath: usize,
    /// The segment within it.
    pub segment: usize,
}

/// What a point of the written path is in node terms.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PointRole {
    /// A node's on-curve point.
    Node(NodeRef),
    /// One of a node's handles.
    Control(NodeRef, Side),
}

/// The closest segment to a query point.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct SegmentHit {
    /// Which segment.
    pub seg: SegRef,
    /// The segment parameter, `0.0..=1.0`.
    pub t: f64,
    /// The closest point.
    pub point: Point,
    /// Its distance from the query, in millipoints.
    pub distance: f64,
}

/// A path as nodes and handles; see the module documentation.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EditPath {
    /// The subpaths.
    pub subpaths: Vec<EditSubpath>,
    /// Whether the written path carries a flag array even when every flag
    /// is empty, because the source did.
    keep_flags: bool,
}

fn f64_pt(p: Point) -> kurbo::Point {
    p.to_kurbo()
}

fn round(p: kurbo::Point) -> Point {
    Point::from_f64_round(p.x, p.y).clamp_to_extent().0
}

impl EditPath {
    /// The node view of a path.
    #[must_use]
    pub fn from_path(path: &Path) -> EditPath {
        let pts = path.points();
        let flag = |i: usize| path.flags_at(i);
        let mut subs: Vec<EditSubpath> = Vec::new();
        let mut cur: Option<EditSubpath> = None;
        let mut i = 0usize;
        for &v in path.verbs() {
            match v {
                Verb::MoveTo => {
                    if let Some(s) = cur.take() {
                        subs.push(s);
                    }
                    let Some(&p) = pts.get(i) else { break };
                    cur = Some(EditSubpath::open(vec![EditNode {
                        at: p,
                        flags: flag(i),
                        ctrl_in: None,
                        ctrl_out: None,
                    }]));
                    i += 1;
                }
                Verb::LineTo => {
                    let (Some(s), Some(&p)) = (cur.as_mut(), pts.get(i)) else {
                        break;
                    };
                    s.nodes.push(EditNode {
                        at: p,
                        flags: flag(i),
                        ctrl_in: None,
                        ctrl_out: None,
                    });
                    i += 1;
                }
                Verb::CubicTo => {
                    let (Some(s), Some(&[c1, c2, p])) = (cur.as_mut(), pts.get(i..i + 3)) else {
                        break;
                    };
                    let Some(last) = s.nodes.last_mut() else {
                        break;
                    };
                    last.ctrl_out = Some(Control {
                        at: c1,
                        flags: flag(i),
                    });
                    s.nodes.push(EditNode {
                        at: p,
                        flags: flag(i + 2),
                        ctrl_in: Some(Control {
                            at: c2,
                            flags: flag(i + 1),
                        }),
                        ctrl_out: None,
                    });
                    i += 3;
                }
                Verb::Close => {
                    let Some(mut s) = cur.take() else { continue };
                    s.closed = true;
                    let n = s.nodes.len();
                    if n > 1 && s.nodes[n - 1].at == s.nodes[0].at {
                        // The `.xar` way: the closing segment ends on a
                        // repeat of the first point. Fold it into node 0.
                        if let Some(last) = s.nodes.pop() {
                            s.nodes[0].ctrl_in = last.ctrl_in;
                            s.close_flags = last.flags;
                        }
                        s.explicit_close = true;
                    }
                    subs.push(s);
                }
            }
        }
        if let Some(s) = cur.take() {
            subs.push(s);
        }
        EditPath {
            subpaths: subs,
            keep_flags: !path.flags().is_empty(),
        }
    }

    /// Writes the nodes back as a path. Subpaths with fewer than two
    /// nodes are dropped: they draw nothing and a lone `MoveTo` in the
    /// middle of a path is not well formed.
    #[must_use]
    pub fn to_path(&self) -> Path {
        let mut verbs = Vec::new();
        let mut points = Vec::new();
        let mut flags = Vec::new();
        self.write(|v, p| {
            if let Some(v) = v {
                verbs.push(v);
            }
            if let Some((p, f)) = p {
                points.push(p.clamp_to_extent().0);
                flags.push(f);
            }
        });
        let any = flags.iter().any(|f| !f.is_empty());
        if !(any || self.keep_flags) {
            flags.clear();
        }
        let path = Path::from_parts(verbs, points, flags);
        debug_assert!(path.is_ok(), "{path:?}");
        path.unwrap_or_default()
    }

    /// The point roles of [`EditPath::to_path`]'s points, in order.
    #[must_use]
    pub fn layout(&self) -> Vec<PointRole> {
        let mut out = Vec::new();
        let mut roles = self.roles();
        self.write(|_, p| {
            if p.is_some()
                && let Some(r) = roles.next()
            {
                out.push(r);
            }
        });
        out
    }

    fn roles(&self) -> impl Iterator<Item = PointRole> + '_ {
        self.subpaths
            .iter()
            .enumerate()
            .filter(|(_, s)| s.nodes.len() >= 2)
            .flat_map(|(si, s)| {
                let n = s.nodes.len();
                let node = move |i: usize| NodeRef::new(si, i);
                let mut v = vec![PointRole::Node(node(0))];
                for k in 1..n {
                    if s.nodes[k - 1].ctrl_out.is_some() {
                        v.push(PointRole::Control(node(k - 1), Side::Out));
                        v.push(PointRole::Control(node(k), Side::In));
                    }
                    v.push(PointRole::Node(node(k)));
                }
                if s.closed {
                    let last = &s.nodes[n - 1];
                    if last.ctrl_out.is_some() {
                        v.push(PointRole::Control(node(n - 1), Side::Out));
                        v.push(PointRole::Control(node(0), Side::In));
                        v.push(PointRole::Node(node(0)));
                    } else if s.explicit_close {
                        v.push(PointRole::Node(node(0)));
                    }
                }
                v
            })
    }

    /// The shared writer: `emit(verb, point)` once per verb and once per
    /// point, in order.
    fn write(&self, mut emit: impl FnMut(Option<Verb>, Option<(Point, PointFlags)>)) {
        for s in self.subpaths.iter().filter(|s| s.nodes.len() >= 2) {
            let n = s.nodes.len();
            let first = &s.nodes[0];
            emit(Some(Verb::MoveTo), Some((first.at, first.flags)));
            let seg = |emit: &mut dyn FnMut(Option<Verb>, Option<(Point, PointFlags)>),
                       a: &EditNode,
                       b: &EditNode,
                       end_flags: PointFlags| {
                if let Some(o) = a.ctrl_out {
                    let i = b.ctrl_in.unwrap_or(Control::at(b.at));
                    emit(Some(Verb::CubicTo), Some((o.at, o.flags)));
                    emit(None, Some((i.at, i.flags)));
                    emit(None, Some((b.at, end_flags)));
                } else {
                    emit(Some(Verb::LineTo), Some((b.at, end_flags)));
                }
            };
            for k in 1..n {
                seg(&mut emit, &s.nodes[k - 1], &s.nodes[k], s.nodes[k].flags);
            }
            if s.closed {
                let last = &s.nodes[n - 1];
                if last.ctrl_out.is_some() || s.explicit_close {
                    let f = if s.explicit_close {
                        s.close_flags
                    } else {
                        first.flags
                    };
                    seg(&mut emit, last, first, f);
                }
                emit(Some(Verb::Close), None);
            }
        }
    }

    /// The index in [`Path::points`] of a node's on-curve point, in the
    /// path [`EditPath::to_path`] writes.
    #[must_use]
    pub fn node_point_index(&self, node: NodeRef) -> Option<u32> {
        self.layout()
            .iter()
            .position(|r| *r == PointRole::Node(node))
            .and_then(|i| u32::try_from(i).ok())
    }

    /// The node whose on-curve point (or a repeat of it) is point `index`.
    #[must_use]
    pub fn node_at_point_index(&self, index: u32) -> Option<NodeRef> {
        match self.layout().get(index as usize)? {
            PointRole::Node(n) => Some(*n),
            PointRole::Control(..) => None,
        }
    }

    /// Every node, in order.
    pub fn node_refs(&self) -> impl Iterator<Item = NodeRef> + '_ {
        self.subpaths
            .iter()
            .enumerate()
            .flat_map(|(si, s)| (0..s.nodes.len()).map(move |i| NodeRef::new(si, i)))
    }

    /// Every segment, in order.
    pub fn seg_refs(&self) -> impl Iterator<Item = SegRef> + '_ {
        self.subpaths.iter().enumerate().flat_map(|(si, s)| {
            (0..s.segment_count()).map(move |i| SegRef {
                subpath: si,
                segment: i,
            })
        })
    }

    /// A node.
    #[must_use]
    pub fn node(&self, r: NodeRef) -> Option<&EditNode> {
        self.subpaths.get(r.subpath)?.nodes.get(r.node)
    }

    /// A node, mutably.
    pub fn node_mut(&mut self, r: NodeRef) -> Option<&mut EditNode> {
        self.subpaths.get_mut(r.subpath)?.nodes.get_mut(r.node)
    }

    /// The two nodes a segment joins.
    #[must_use]
    pub fn ends(&self, s: SegRef) -> Option<(NodeRef, NodeRef)> {
        let sub = self.subpaths.get(s.subpath)?;
        if s.segment >= sub.segment_count() {
            return None;
        }
        let b = sub.next(s.segment)?;
        Some((
            NodeRef::new(s.subpath, s.segment),
            NodeRef::new(s.subpath, b),
        ))
    }

    /// The segment leaving a node, if any.
    #[must_use]
    pub fn seg_after(&self, n: NodeRef) -> Option<SegRef> {
        let sub = self.subpaths.get(n.subpath)?;
        (n.node < sub.segment_count()).then_some(SegRef {
            subpath: n.subpath,
            segment: n.node,
        })
    }

    /// The segment arriving at a node, if any.
    #[must_use]
    pub fn seg_before(&self, n: NodeRef) -> Option<SegRef> {
        let sub = self.subpaths.get(n.subpath)?;
        let p = sub.prev(n.node)?;
        (p < sub.segment_count()).then_some(SegRef {
            subpath: n.subpath,
            segment: p,
        })
    }

    /// A segment's geometry.
    #[must_use]
    pub fn segment(&self, s: SegRef) -> Option<Segment> {
        let (a, b) = self.ends(s)?;
        let (a, b) = (self.node(a)?, self.node(b)?);
        Some(match a.ctrl_out {
            Some(o) => Segment::Cubic {
                p0: a.at,
                p1: o.at,
                p2: b.ctrl_in.map_or(b.at, |c| c.at),
                p3: b.at,
            },
            None => Segment::Line { p0: a.at, p1: b.at },
        })
    }

    /// Whether a segment is a curve.
    #[must_use]
    pub fn is_curve(&self, s: SegRef) -> bool {
        matches!(self.segment(s), Some(Segment::Cubic { .. }))
    }

    /// The segment closest to `p`, with the parameter and distance there.
    #[must_use]
    pub fn nearest_segment(&self, p: Point) -> Option<SegmentHit> {
        let target = f64_pt(p);
        let mut best: Option<SegmentHit> = None;
        for s in self.seg_refs() {
            let Some(seg) = self.segment(s) else { continue };
            let k = seg.to_kurbo();
            let n = k.nearest(target, 1e-3);
            let d = n.distance_sq.sqrt();
            if best.as_ref().is_none_or(|b| d < b.distance) {
                best = Some(SegmentHit {
                    seg: s,
                    t: n.t,
                    point: round(k.eval(n.t)),
                    distance: d,
                });
            }
        }
        best
    }

    /// Moves a node and both its handles.
    pub fn move_node(&mut self, n: NodeRef, by: Vector) {
        if let Some(node) = self.node_mut(n) {
            node.translate(by);
        }
    }

    /// Moves one handle to `to`, by hand: the handle, its node and the
    /// node's other handle stop being auto-placed. When the node is smooth
    /// and `keep_smooth` holds, the opposite handle turns to stay
    /// collinear, keeping its own length.
    pub fn move_control(&mut self, n: NodeRef, side: Side, to: Point, keep_smooth: bool) {
        let Some(node) = self.node_mut(n) else { return };
        if node.control(side).is_none() {
            return;
        }
        node.flags.remove(PointFlags::SMOOTH);
        for s in [Side::In, Side::Out] {
            if let Some(c) = node.control_mut(s) {
                c.flags.remove(PointFlags::SMOOTH);
            }
        }
        if let Some(c) = node.control_mut(side) {
            c.at = to.clamp_to_extent().0;
        }
        if !(keep_smooth && node.is_smooth()) {
            return;
        }
        let at = node.at;
        let Some(other) = node.control_mut(side.opposite()) else {
            return;
        };
        let (dx, dy) = (to - at).to_f64();
        let l = dx.hypot(dy);
        let keep = (other.at - at).length();
        if l < 0.5 || keep < 0.5 {
            return;
        }
        let (ax, ay) = at.to_f64();
        other.at = Point::from_f64_round(ax - dx / l * keep, ay - dy / l * keep)
            .clamp_to_extent()
            .0;
    }

    /// Splits a segment at parameter `t`, keeping the curve where it was.
    /// Returns the new node, which is smooth when the segment was a curve.
    pub fn split(&mut self, s: SegRef, t: f64) -> Option<NodeRef> {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let seg = self.segment(s)?;
        let (a, _) = self.ends(s)?;
        let sub = self.subpaths.get_mut(s.subpath)?;
        let at = s.segment + 1;
        let node = match seg {
            Segment::Line { p0, p1 } => {
                EditNode::corner(round(kurbo::Line::new(f64_pt(p0), f64_pt(p1)).eval(t)))
            }
            Segment::Cubic { p0, p1, p2, p3 } => {
                let c = kurbo::CubicBez::new(f64_pt(p0), f64_pt(p1), f64_pt(p2), f64_pt(p3));
                let (l, r) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
                if let Some(o) = &mut sub.nodes[a.node].ctrl_out {
                    o.at = round(l.p1);
                }
                let b = sub.next(a.node)?;
                if let Some(i) = &mut sub.nodes[b].ctrl_in {
                    i.at = round(r.p2);
                }
                EditNode {
                    at: round(l.p3),
                    flags: PointFlags::END_POINT | PointFlags::ROTATE,
                    ctrl_in: Some(Control::at(round(l.p2))),
                    ctrl_out: Some(Control::at(round(r.p1))),
                }
            }
        };
        sub.nodes.insert(at, node);
        Some(NodeRef::new(s.subpath, at))
    }

    /// Deletes nodes, joining the segments either side of each into one
    /// with the outer handles' directions kept. A subpath left with fewer
    /// than two nodes is removed. Returns whether anything is left.
    pub fn delete(&mut self, nodes: &[NodeRef]) -> bool {
        let mut nodes = nodes.to_vec();
        nodes.sort_unstable();
        nodes.dedup();
        for r in nodes.into_iter().rev() {
            self.delete_one(r);
        }
        self.subpaths.retain(|s| s.nodes.len() >= 2);
        !self.subpaths.is_empty()
    }

    fn delete_one(&mut self, r: NodeRef) {
        let Some(sub) = self.subpaths.get_mut(r.subpath) else {
            return;
        };
        let n = sub.nodes.len();
        if r.node >= n {
            return;
        }
        let (prev, next) = (sub.prev(r.node), sub.next(r.node));
        match (prev, next) {
            (Some(a), Some(b)) if a != b || n > 2 => {
                let merged = merge(&sub.nodes[a], &sub.nodes[r.node], &sub.nodes[b]);
                sub.nodes[a].ctrl_out = merged.map(|(o, _)| Control {
                    at: o,
                    flags: sub.nodes[a]
                        .ctrl_out
                        .map_or(PointFlags::empty(), |c| c.flags),
                });
                sub.nodes[b].ctrl_in = merged.map(|(_, i)| Control {
                    at: i,
                    flags: sub.nodes[b]
                        .ctrl_in
                        .map_or(PointFlags::empty(), |c| c.flags),
                });
                sub.nodes.remove(r.node);
            }
            (None, Some(b)) => {
                sub.nodes[b].ctrl_in = None;
                sub.nodes.remove(r.node);
            }
            (Some(a), None) => {
                sub.nodes[a].ctrl_out = None;
                sub.nodes.remove(r.node);
            }
            _ => {
                sub.nodes.remove(r.node);
            }
        }
        if sub.nodes.len() < 3 && sub.closed {
            // Two nodes cannot enclose anything with a line and a curve
            // back; keep it closed only while it still has an area.
            let closing_curve = sub.nodes.last().is_some_and(|l| l.ctrl_out.is_some());
            if sub.nodes.len() < 2 || !closing_curve && sub.nodes[0].ctrl_out.is_none() {
                sub.closed = false;
                sub.explicit_close = false;
                if let Some(l) = sub.nodes.last_mut() {
                    l.ctrl_out = None;
                }
                if let Some(f) = sub.nodes.first_mut() {
                    f.ctrl_in = None;
                }
            }
        }
    }

    /// Makes a segment straight.
    pub fn make_line(&mut self, s: SegRef) {
        let Some((a, b)) = self.ends(s) else { return };
        if let Some(n) = self.node_mut(a) {
            n.ctrl_out = None;
        }
        if let Some(n) = self.node_mut(b) {
            n.ctrl_in = None;
        }
    }

    /// Makes a straight segment a curve. Its handles start a third and
    /// two thirds of the way along and are auto-placed, so they settle
    /// along the neighbouring segments' tangents. A curve is left as it is.
    pub fn make_curve(&mut self, s: SegRef) {
        let Some((a, b)) = self.ends(s) else { return };
        if self.is_curve(s) {
            return;
        }
        let (Some(pa), Some(pb)) = (self.node(a).map(|n| n.at), self.node(b).map(|n| n.at)) else {
            return;
        };
        let (x0, y0) = pa.to_f64();
        let (x1, y1) = pb.to_f64();
        let third = |k: f64| Control {
            at: Point::from_f64_round(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k),
            flags: PointFlags::SMOOTH | PointFlags::ROTATE,
        };
        if let Some(n) = self.node_mut(a) {
            n.ctrl_out = Some(third(1.0 / 3.0));
        }
        if let Some(n) = self.node_mut(b) {
            n.ctrl_in = Some(third(2.0 / 3.0));
        }
        self.auto_place(a, Side::Out);
        self.auto_place(b, Side::In);
    }

    /// Makes a node smooth.
    ///
    /// Handles that are already collinear stay exactly where they are and
    /// the node just becomes smooth, so smooth → cusp → smooth is the
    /// identity on a smooth node. Otherwise the node and its handles are
    /// marked auto-placed and placed: along the tangent perpendicular to
    /// the bisector of the neighbours' directions when both sides are
    /// curves, along the straight segment when one side is a line, each a
    /// third of its own segment's chord long. Straight segments stay
    /// straight.
    pub fn make_smooth(&mut self, n: NodeRef) {
        let Some(node) = self.node(n) else { return };
        let already = match (node.ctrl_in, node.ctrl_out) {
            (Some(i), Some(o)) => collinear(i.at, node.at, o.at),
            _ => false,
        };
        let Some(node) = self.node_mut(n) else { return };
        node.flags |= PointFlags::ROTATE;
        if already {
            return;
        }
        node.flags |= PointFlags::SMOOTH;
        for s in [Side::In, Side::Out] {
            if let Some(c) = node.control_mut(s) {
                c.flags |= PointFlags::SMOOTH | PointFlags::ROTATE;
            }
        }
        self.auto_place(n, Side::In);
        self.auto_place(n, Side::Out);
    }

    /// Makes a node a cusp: its handles move independently and are no
    /// longer auto-placed. They stay where they are.
    pub fn make_cusp(&mut self, n: NodeRef) {
        if let Some(node) = self.node_mut(n) {
            node.flags.remove(PointFlags::SMOOTH | PointFlags::ROTATE);
            for s in [Side::In, Side::Out] {
                if let Some(c) = node.control_mut(s) {
                    c.flags.remove(PointFlags::SMOOTH | PointFlags::ROTATE);
                }
            }
        }
    }

    /// Recomputes every auto-placed handle of the given nodes and of their
    /// neighbours: what moving those nodes owes the curve.
    pub fn resmooth_around(&mut self, nodes: &[NodeRef]) {
        let mut todo: Vec<NodeRef> = Vec::new();
        for &r in nodes {
            let Some(sub) = self.subpaths.get(r.subpath) else {
                continue;
            };
            todo.push(r);
            todo.extend(sub.prev(r.node).map(|i| NodeRef::new(r.subpath, i)));
            todo.extend(sub.next(r.node).map(|i| NodeRef::new(r.subpath, i)));
        }
        todo.sort_unstable();
        todo.dedup();
        for r in todo {
            for side in [Side::In, Side::Out] {
                let auto = self
                    .node(r)
                    .and_then(|n| n.control(side))
                    .is_some_and(|c| c.flags.contains(PointFlags::SMOOTH));
                if auto {
                    self.auto_place(r, side);
                }
            }
        }
    }

    /// Puts one handle where the auto-placement rule says.
    fn auto_place(&mut self, r: NodeRef, side: Side) {
        let Some(sub) = self.subpaths.get(r.subpath) else {
            return;
        };
        let Some(node) = sub.nodes.get(r.node) else {
            return;
        };
        if node.control(side).is_none() {
            return;
        }
        let (far, other) = match side {
            Side::Out => (sub.next(r.node), sub.prev(r.node)),
            Side::In => (sub.prev(r.node), sub.next(r.node)),
        };
        let Some(far) = far.map(|i| sub.nodes[i].at) else {
            return;
        };
        let at = node.at;
        let unit = |v: Vector| -> Option<(f64, f64)> {
            let (x, y) = v.to_f64();
            let l = x.hypot(y);
            (l > 0.0).then(|| (x / l, y / l))
        };
        let other_curve = node.control(side.opposite()).is_some();
        let dir = match other.map(|i| sub.nodes[i].at) {
            // Both sides curves: perpendicular to the bisector of the
            // directions to the two neighbours, towards `far`.
            Some(o) if other_curve => match (unit(far - at), unit(o - at)) {
                (Some(f), Some(b)) => {
                    let (x, y) = (f.0 - b.0, f.1 - b.1);
                    let l = x.hypot(y);
                    if l > 1e-9 {
                        Some((x / l, y / l))
                    } else {
                        Some(f)
                    }
                }
                (f, _) => f,
            },
            // The other side is straight: carry the line on.
            Some(o) => unit(at - o).or_else(|| unit(far - at)),
            // An open end: along the chord.
            None => unit(far - at),
        };
        let Some((dx, dy)) = dir else { return };
        let len = (far - at).length() / 3.0;
        let (ax, ay) = at.to_f64();
        let p = Point::from_f64_round(ax + dx * len, ay + dy * len)
            .clamp_to_extent()
            .0;
        if let Some(node) = self.node_mut(r)
            && let Some(c) = node.control_mut(side)
        {
            c.at = p;
        }
    }

    /// Closes an open subpath with a straight segment from its last node
    /// to its first. When the two ends coincide they become one node.
    pub fn close(&mut self, subpath: usize) {
        let Some(s) = self.subpaths.get_mut(subpath) else {
            return;
        };
        let n = s.nodes.len();
        if s.closed || n < 2 {
            return;
        }
        s.closed = true;
        s.explicit_close = false;
        if n > 2 && s.nodes[n - 1].at == s.nodes[0].at {
            if let Some(last) = s.nodes.pop() {
                s.nodes[0].ctrl_in = last.ctrl_in;
                s.close_flags = last.flags;
            }
            s.explicit_close = true;
        }
    }

    /// Opens a closed subpath by removing its closing segment. The nodes
    /// are kept, so closing and opening again keeps the point count.
    pub fn open(&mut self, subpath: usize) {
        let Some(s) = self.subpaths.get_mut(subpath) else {
            return;
        };
        if !s.closed {
            return;
        }
        s.closed = false;
        s.explicit_close = false;
        if let Some(l) = s.nodes.last_mut() {
            l.ctrl_out = None;
        }
        if let Some(f) = s.nodes.first_mut() {
            f.ctrl_in = None;
        }
    }

    /// Breaks the path at a node: a closed subpath opens there, starting
    /// and ending on copies of the node; an open one splits in two, both
    /// ending on a copy. An end node of an open subpath is left alone.
    /// Returns the copies (the original first).
    pub fn break_at(&mut self, r: NodeRef) -> Option<(NodeRef, NodeRef)> {
        let s = self.subpaths.get_mut(r.subpath)?;
        let n = s.nodes.len();
        if r.node >= n {
            return None;
        }
        if s.closed {
            s.nodes.rotate_left(r.node);
            let mut head = s.nodes[0].clone();
            let mut tail = head.clone();
            head.ctrl_in = None;
            tail.ctrl_out = None;
            s.nodes[0] = head;
            s.nodes.push(tail);
            s.closed = false;
            s.explicit_close = false;
            return Some((
                NodeRef::new(r.subpath, 0),
                NodeRef::new(r.subpath, s.nodes.len() - 1),
            ));
        }
        if r.node == 0 || r.node + 1 >= n {
            return None;
        }
        let mut rest = s.nodes.split_off(r.node);
        let mut end = rest[0].clone();
        end.ctrl_out = None;
        rest[0].ctrl_in = None;
        s.nodes.push(end);
        let tail = EditSubpath::open(rest);
        self.subpaths.insert(r.subpath + 1, tail);
        Some((
            NodeRef::new(r.subpath, r.node),
            NodeRef::new(r.subpath + 1, 0),
        ))
    }

    /// Joins two end nodes of open subpaths with a straight segment, or
    /// merges them when they coincide. Two ends of one subpath close it.
    /// Returns whether anything changed.
    pub fn join(&mut self, a: NodeRef, b: NodeRef) -> bool {
        let is_end = |p: &EditPath, r: NodeRef| {
            p.subpaths
                .get(r.subpath)
                .is_some_and(|s| !s.closed && (r.node == 0 || r.node + 1 == s.nodes.len()))
        };
        if a == b || !is_end(self, a) || !is_end(self, b) {
            return false;
        }
        if a.subpath == b.subpath {
            self.close(a.subpath);
            return true;
        }
        // Orient: `first` must end at `a`, `second` start at `b`.
        let mut first = self.subpaths[a.subpath].clone();
        let mut second = self.subpaths[b.subpath].clone();
        if a.node == 0 && first.nodes.len() > 1 {
            reverse_nodes(&mut first.nodes);
        }
        if b.node != 0 {
            reverse_nodes(&mut second.nodes);
        }
        let same = first.nodes.last().map(|n| n.at) == second.nodes.first().map(|n| n.at);
        let mut rest = second.nodes;
        if same && !rest.is_empty() {
            let head = rest.remove(0);
            if let Some(l) = first.nodes.last_mut() {
                l.ctrl_out = head.ctrl_out;
            }
        }
        first.nodes.extend(rest);
        let (lo, hi) = if a.subpath < b.subpath {
            (a.subpath, b.subpath)
        } else {
            (b.subpath, a.subpath)
        };
        self.subpaths.remove(hi);
        self.subpaths[lo] = first;
        true
    }
}

/// Reverses a run of nodes, swapping each node's handles.
fn reverse_nodes(nodes: &mut [EditNode]) {
    nodes.reverse();
    for n in nodes {
        std::mem::swap(&mut n.ctrl_in, &mut n.ctrl_out);
    }
}

/// Whether handles at `a` and `c` are collinear through the node `b`,
/// on opposite sides, to within the millipoint rounding of their ends. A
/// handle lying on its node has no direction and is collinear with
/// anything.
fn collinear(a: Point, b: Point, c: Point) -> bool {
    let (ax, ay) = (a - b).to_f64();
    let (cx, cy) = (c - b).to_f64();
    let (la, lc) = (ax.hypot(ay), cx.hypot(cy));
    if la < 0.5 || lc < 0.5 {
        return true;
    }
    let sin = (ax * cy - ay * cx).abs() / (la * lc);
    let dot = ax * cx + ay * cy;
    // The node and each handle may be half a unit off in each axis: allow
    // the angle that makes at either length.
    dot < 0.0 && sin <= 2.0 / la + 2.0 / lc
}

/// The handles of the one cubic replacing `a → m → b`, or `None` for a
/// straight segment: joining a line to anything gives a line
/// (`research/04 §4.11`); only two curves give a curve.
fn merge(a: &EditNode, m: &EditNode, b: &EditNode) -> Option<(Point, Point)> {
    let (q1, r1) = (a.ctrl_out?, m.ctrl_out?);
    let (p0, pm, p3) = (f64_pt(a.at), f64_pt(m.at), f64_pt(b.at));
    let (q1, r1) = (f64_pt(q1.at), f64_pt(r1.at));
    let q2 = m.ctrl_in.map_or(pm, |c| f64_pt(c.at));
    let r2 = b.ctrl_in.map_or(p3, |c| f64_pt(c.at));
    // Where along the merged curve the deleted node was. For a node that a
    // split made, its handles are in the ratio t : 1 - t; rounding blurs
    // that when they are short, so the estimate is refined by asking which
    // t, split again, reproduces the deleted node's handles best.
    let (hl, hr) = ((pm - q2).hypot(), (r1 - pm).hypot());
    let guess = if hl > 1e-9 && hr > 1e-9 {
        hl / (hl + hr)
    } else {
        let l = kurbo::CubicBez::new(p0, q1, q2, pm).arclen(1e-3);
        let r = kurbo::CubicBez::new(pm, r1, r2, p3).arclen(1e-3);
        if l + r > 0.0 { l / (l + r) } else { 0.5 }
    }
    .clamp(0.05, 0.95);
    let rebuilt = |t: f64| {
        let p1 = p0 + (q1 - p0) / t;
        let p2 = p3 + (r2 - p3) / (1.0 - t);
        kurbo::CubicBez::new(p0, p1, p2, p3)
    };
    let error = |t: f64| {
        let c = rebuilt(t);
        let (l, r) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
        (l.p2 - q2).hypot() + (l.p3 - pm).hypot() + (r.p1 - r1).hypot()
    };
    let mut best = guess;
    let mut best_e = error(guess);
    for i in 0..=90 {
        let t = 0.05 + f64::from(i) * 0.01;
        let e = error(t);
        if e < best_e {
            (best, best_e) = (t, e);
        }
    }
    // Golden-section search around the best sample.
    let (mut lo, mut hi) = ((best - 0.01).max(0.05), (best + 0.01).min(0.95));
    let g = 0.618_033_988_749_895;
    for _ in 0..40 {
        let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if error(a) < error(b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    let t = if error(f64::midpoint(lo, hi)) < best_e {
        f64::midpoint(lo, hi)
    } else {
        best
    };
    let c = rebuilt(t);
    let (p1, p2) = (c.p1, c.p2);
    Some((round(p1), round(p2)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathBuilder;

    fn pt(x: i32, y: i32) -> Point {
        Point::raw(x, y)
    }

    fn wave() -> Path {
        let mut b = PathBuilder::new();
        b.move_to(pt(0, 0));
        b.cubic_to(pt(0, 30_000), pt(60_000, 30_000), pt(60_000, 0));
        b.line_to(pt(90_000, 0));
        b.build()
    }

    fn xar_closed() -> Path {
        // The `.xar` way: the last point repeats the first, then Close.
        Path::from_parts(
            vec![
                Verb::MoveTo,
                Verb::LineTo,
                Verb::CubicTo,
                Verb::LineTo,
                Verb::Close,
            ],
            vec![
                pt(0, 0),
                pt(10_000, 0),
                pt(15_000, 5_000),
                pt(15_000, 10_000),
                pt(10_000, 15_000),
                pt(0, 0),
            ],
            vec![PointFlags::END_POINT; 6],
        )
        .unwrap()
    }

    #[test]
    fn the_round_trip_is_exact_for_both_ways_of_closing() {
        for p in [wave(), xar_closed(), {
            let mut b = PathBuilder::new();
            b.rect(crate::Rect::new(pt(0, 0), pt(5, 5)));
            b.ellipse(pt(100, 100), crate::Mp::new(50), crate::Mp::new(20));
            b.build()
        }] {
            let e = EditPath::from_path(&p);
            assert_eq!(e.to_path(), p);
            assert_eq!(e.layout().len(), p.points().len());
        }
        let e = EditPath::from_path(&xar_closed());
        assert_eq!(e.subpaths[0].nodes.len(), 3, "the repeat is folded");
        // The repeated last point is node 0 as well.
        assert_eq!(e.node_at_point_index(5), Some(NodeRef::new(0, 0)));
        assert_eq!(e.node_at_point_index(2), None, "a control point");
        assert_eq!(e.node_point_index(NodeRef::new(0, 2)), Some(4));
    }

    #[test]
    fn splitting_keeps_the_curve_and_deleting_the_new_node_restores_it() {
        let p = wave();
        let mut e = EditPath::from_path(&p);
        let s = SegRef {
            subpath: 0,
            segment: 0,
        };
        let before = e.segment(s).unwrap().to_kurbo();
        let n = e.split(s, 0.3).unwrap();
        assert!(e.node(n).unwrap().is_smooth());
        // Both halves lie on the old curve.
        for seg in [s, SegRef { segment: 1, ..s }] {
            let k = e.segment(seg).unwrap().to_kurbo();
            for i in 0..=10 {
                let q = k.eval(f64::from(i) / 10.0);
                assert!(before.nearest(q, 1e-6).distance_sq.sqrt() < 2.0);
            }
        }
        e.delete(&[n]);
        let after = e.to_path();
        for (a, b) in after.points().iter().zip(p.points()) {
            assert!(a.distance_to(*b) <= 6.0, "{a:?} {b:?}");
        }
        assert_eq!(after.verbs(), p.verbs());
    }

    #[test]
    fn splitting_and_deleting_on_a_line_is_exact() {
        let p = wave();
        let mut e = EditPath::from_path(&p);
        let n = e
            .split(
                SegRef {
                    subpath: 0,
                    segment: 1,
                },
                0.5,
            )
            .unwrap();
        assert_eq!(e.node(n).unwrap().at, pt(75_000, 0));
        e.delete(&[n]);
        assert_eq!(e.to_path(), p);
    }

    #[test]
    fn smooth_cusp_smooth_is_the_identity_on_a_smooth_node() {
        let mut e = EditPath::from_path(&wave());
        let n = e
            .split(
                SegRef {
                    subpath: 0,
                    segment: 0,
                },
                0.5,
            )
            .unwrap();
        let smooth = e.clone();
        e.make_cusp(n);
        assert!(!e.node(n).unwrap().is_smooth());
        e.make_smooth(n);
        assert_eq!(e, smooth);
    }

    #[test]
    fn smoothing_a_corner_carries_the_line_on_into_the_curve() {
        let mut e = EditPath::from_path(&wave());
        // Node 1 joins the curve and the line at a corner.
        let n = NodeRef::new(0, 1);
        e.make_smooth(n);
        let node = e.node(n).unwrap().clone();
        assert!(node.is_smooth());
        assert!(node.ctrl_out.is_none(), "the line stays a line");
        let i = node.ctrl_in.unwrap();
        assert!(i.flags.contains(PointFlags::SMOOTH), "auto-placed");
        // Collinear with the line, a third of the curve's chord out.
        assert_eq!(i.at, pt(40_000, 0));
        // Moving the node's neighbour re-places the auto handle.
        e.move_node(NodeRef::new(0, 2), Vector::raw(0, 30_000));
        e.resmooth_around(&[NodeRef::new(0, 2)]);
        let i2 = e.node(n).unwrap().ctrl_in.unwrap().at;
        assert!(i2.y.raw() < 0, "turned with the line: {i2:?}");
    }

    #[test]
    fn dragging_a_handle_of_a_smooth_node_turns_the_other() {
        let mut e = EditPath::from_path(&wave());
        let n = e
            .split(
                SegRef {
                    subpath: 0,
                    segment: 0,
                },
                0.5,
            )
            .unwrap();
        let node = e.node(n).unwrap().clone();
        let keep = (node.ctrl_in.unwrap().at - node.at).length();
        e.move_control(n, Side::Out, node.at + Vector::raw(10_000, 10_000), true);
        let after = e.node(n).unwrap();
        let (i, o) = (after.ctrl_in.unwrap().at, after.ctrl_out.unwrap().at);
        assert!(collinear(i, after.at, o), "{i:?} {:?} {o:?}", after.at);
        assert!(((i - after.at).length() - keep).abs() < 1.5);
        // A cusp's handles move alone.
        e.make_cusp(n);
        e.move_control(n, Side::Out, node.at + Vector::raw(0, 10_000), true);
        assert_eq!(e.node(n).unwrap().ctrl_in.unwrap().at, i);
    }

    #[test]
    fn make_line_and_make_curve_toggle_a_segment() {
        let mut e = EditPath::from_path(&wave());
        let s = SegRef {
            subpath: 0,
            segment: 1,
        };
        e.make_curve(s);
        assert!(e.is_curve(s));
        assert_eq!(
            e.node(NodeRef::new(0, 1)).unwrap().ctrl_out.unwrap().at,
            pt(70_000, 0)
        );
        e.make_line(s);
        assert_eq!(e.to_path(), wave());
    }

    #[test]
    fn closing_then_opening_keeps_the_point_count() {
        let p = wave();
        let mut e = EditPath::from_path(&p);
        e.close(0);
        assert!(e.subpaths[0].closed);
        assert_eq!(e.to_path().points().len(), p.points().len());
        e.open(0);
        assert_eq!(e.to_path(), p);
    }

    #[test]
    fn breaking_a_closed_path_opens_it_at_the_node() {
        let mut e = EditPath::from_path(&xar_closed());
        let (a, b) = e.break_at(NodeRef::new(0, 2)).unwrap();
        assert!(!e.subpaths[0].closed);
        assert_eq!(e.node(a).unwrap().at, e.node(b).unwrap().at);
        assert_eq!(e.subpaths[0].nodes.len(), 4);
        // And an open one splits in two.
        let mut e = EditPath::from_path(&wave());
        e.break_at(NodeRef::new(0, 1)).unwrap();
        assert_eq!(e.subpaths.len(), 2);
        assert_eq!(e.to_path().subpaths().count(), 2);
    }

    #[test]
    fn joining_two_ends_makes_one_subpath() {
        let mut b = PathBuilder::new();
        b.move_to(pt(0, 0)).line_to(pt(10, 0));
        b.move_to(pt(30, 0)).line_to(pt(20, 0));
        let mut e = EditPath::from_path(&b.build());
        assert!(e.join(NodeRef::new(0, 1), NodeRef::new(1, 1)));
        assert_eq!(e.subpaths.len(), 1);
        let pts: Vec<Point> = e.subpaths[0].nodes.iter().map(|n| n.at).collect();
        assert_eq!(pts, vec![pt(0, 0), pt(10, 0), pt(20, 0), pt(30, 0)]);
        assert!(e.join(NodeRef::new(0, 0), NodeRef::new(0, 3)));
        assert!(e.subpaths[0].closed);
    }

    #[test]
    fn deleting_down_to_one_node_removes_the_subpath() {
        let mut e = EditPath::from_path(&wave());
        assert!(e.delete(&[NodeRef::new(0, 0)]));
        assert!(!e.delete(&[NodeRef::new(0, 0)]));
        assert!(e.to_path().is_empty());
    }

    #[test]
    fn the_nearest_segment_is_found_with_its_parameter() {
        let e = EditPath::from_path(&wave());
        let h = e.nearest_segment(pt(80_000, 500)).unwrap();
        assert_eq!(h.seg.segment, 1);
        assert!((h.t - 2.0 / 3.0).abs() < 1e-3);
        assert!((h.distance - 500.0).abs() < 1.0);
    }
}
