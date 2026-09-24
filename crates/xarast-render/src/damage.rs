//! What changed on screen between two scenes of one document.
//!
//! An edit rebuilds the whole scene (`docs/memory/app-core.md`, decision
//! 24), but it changes the pixels of only a few objects. [`scene_damage`]
//! compares the scene a frame was drawn from with the scene that replaces
//! it and returns the device rectangles whose pixels may differ, so that
//! the render thread repaints those and keeps the rest (XARA-T-0221).
//!
//! # Why a scene diff, and not the command's own extent
//!
//! A command knows the node it touched, but not everything that paints
//! differently because of it: a group's attribute recolours every sibling
//! under it, a named colour every object that uses it, a z-order change the
//! objects it crosses, an undo whatever the step touched. The scene is the
//! one place where all of that has already been resolved into paint, so a
//! diff of it is correct for every command at once, previews and undo
//! included, and needs no per-command bookkeeping that could miss a case.
//!
//! # The model, and why the result is sufficient
//!
//! A scene is a tree: the structural pushes (group, clip, transparency
//! scope, offscreen layer) are inner nodes, the fills, strokes and images
//! are leaves. The diff aligns the children of two inner nodes whose push
//! ops are equal, in order:
//!
//! * a common prefix and suffix are matched pairwise;
//! * the rest is matched by a key and op equality, and the matches are cut
//!   to the longest subsequence whose order agrees in both scenes;
//! * a matched inner node is compared recursively; a matched leaf is
//!   identical (the op compares equal, and so do the ramps and images it
//!   refers to);
//! * every leaf under an unmatched child, on either side, is damage: its
//!   device bounds under the transforms of its ancestors.
//!
//! A pixel `p` outside the damage is only covered by matched leaves in
//! both scenes. They cover it in the same order, under ancestors whose
//! push ops are equal, so `p` is composited from the same inputs in the
//! same order and comes out the same. That rests on two properties of the
//! renderer, which the property tests pin: a leaf touches no pixel outside
//! the device bounds the display list gives it, and a clip, a group or a
//! layer with no leaf covering `p` leaves `p` alone.

use std::collections::HashMap;

use crate::backend::cpu::Resolver;
use crate::blend::{TranspSource, Transparency};
use crate::display_list::{ViewParams, device_bounds_of, mapping_bounds};
use crate::paint::{GradRamp, ImageId, Paint};
use crate::precision::Transform2D;
use crate::ramp::RampId;
use crate::scene::{Scene, SceneOp, stroke_pad};
use crate::surface::DeviceRect;

/// Above this many raw rectangles the damage is first reduced by
/// merging neighbours in reading order, so that the pairwise merge stays
/// cheap.
const PRE_MERGE: usize = 256;

/// What [`scene_damage`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Damage {
    /// Device rectangles, inside the viewport, whose pixels may differ.
    /// Empty when the two scenes draw the same picture.
    pub rects: Vec<DeviceRect>,
    /// Leaves of the old scene that were not matched.
    pub removed: usize,
    /// Leaves of the new scene that were not matched.
    pub added: usize,
}

impl Damage {
    /// Pixels covered, counting overlaps once per rectangle.
    #[must_use]
    pub fn area(&self) -> u64 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// The bounding box of the damage.
    #[must_use]
    pub fn bounds(&self) -> DeviceRect {
        self.rects
            .iter()
            .fold(DeviceRect::EMPTY, |a, r| a.union(*r))
    }
}

/// The device rectangles of `view` whose pixels may differ between a
/// frame of `old` and a frame of `new`, merged into at most `max_rects`.
///
/// `None` when the two cannot be compared: they were recorded at
/// different qualities, or one of them is unbalanced. The caller then
/// redraws everything.
#[must_use]
pub fn scene_damage(
    old: (&Scene, &Resolver),
    new: (&Scene, &Resolver),
    view: &ViewParams,
    max_rects: usize,
) -> Option<Damage> {
    if old.0.quality() != new.0.quality() {
        return None;
    }
    let a = Side::new(old.0, old.1)?;
    let b = Side::new(new.0, new.1)?;
    let mut diff = Diff {
        a,
        b,
        view: view.viewport,
        ramps: HashMap::new(),
        images: HashMap::new(),
        rects: Vec::new(),
        removed: 0,
        added: 0,
        same_resolver: std::ptr::eq(old.1, new.1),
    };
    let root = Work {
        a: (0, diff.a.len()),
        b: (0, diff.b.len()),
        xf: view.transform,
    };
    let mut work = vec![root];
    while let Some(w) = work.pop() {
        diff.children(&w, &mut work);
    }
    Some(Damage {
        rects: coalesce(diff.rects, max_rects.max(1)),
        removed: diff.removed,
        added: diff.added,
    })
}

/// The device rectangles of `view` that a frame of `scene` drew from any
/// image `hit` selects, merged into at most `max_rects`: every leaf that
/// samples one (as a placed image, an image paint or a bitmap
/// transparency) and everything under a transparency scope or layer whose
/// mask is one. The same bounds [`scene_damage`] uses, so repainting them
/// over a frame gives the frame a full render of `scene` gives.
///
/// This is how the render thread redraws what it drew from a smaller
/// resident level while an evicted base came back (`pixel_budget`,
/// "Drawing without waiting", XARA-T-0281). `None` for an unbalanced
/// scene.
#[must_use]
pub fn image_damage(
    scene: &Scene,
    view: &ViewParams,
    hit: impl Fn(ImageId) -> bool,
    max_rects: usize,
) -> Option<Damage> {
    let res = Resolver::new();
    let side = Side::new(scene, &res)?;
    let mut rects = Vec::new();
    let mut leaves = 0;
    let mut stack = vec![view.transform];
    let mut i = 0;
    while i < side.len() {
        let op = side.op(i);
        let xf = stack.last().copied().unwrap_or(view.transform);
        match op {
            SceneOp::PushGroup { xf: g, .. } => {
                stack.push(g.then(xf));
                i += 1;
                continue;
            }
            SceneOp::PopGroup => {
                stack.pop();
                i += 1;
                continue;
            }
            _ => {}
        }
        let mut uses = false;
        refs(op, &mut |r| {
            if let Ref::Image(id) = r {
                uses = uses || hit(id);
            }
        });
        if uses {
            let end = side.close[i as usize].max(i);
            leaves += leaf_bounds(&side, (i, end), xf, view.viewport, &mut rects);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    Some(Damage {
        rects: coalesce(rects, max_rects.max(1)),
        removed: leaves,
        added: leaves,
    })
}

/// One scene, with the matching pop of every push.
struct Side<'a> {
    ops: &'a [SceneOp],
    /// For a push, the index of its pop; for anything else, itself.
    close: Vec<u32>,
    res: &'a Resolver,
}

impl<'a> Side<'a> {
    fn new(scene: &'a Scene, res: &'a Resolver) -> Option<Side<'a>> {
        let ops = scene.ops.as_slice();
        let n = u32::try_from(ops.len()).ok()?;
        let mut close: Vec<u32> = (0..n).collect();
        let mut open: Vec<(u32, u8)> = Vec::new();
        for (i, op) in (0..n).zip(ops) {
            if let Some(k) = push_kind(op) {
                open.push((i, k));
            } else if let Some(k) = pop_kind(op) {
                let (at, kind) = open.pop()?;
                if kind != k {
                    return None;
                }
                close[at as usize] = i;
            }
        }
        open.is_empty().then_some(Side { ops, close, res })
    }

    fn len(&self) -> u32 {
        // `new` checked that the length fits.
        u32::try_from(self.ops.len()).unwrap_or(u32::MAX)
    }

    fn op(&self, i: u32) -> &'a SceneOp {
        &self.ops[i as usize]
    }

    /// The children of the range `[lo, hi)`: each a leaf `(i, i)` or a
    /// subtree `(push, pop)`.
    fn children(&self, (lo, hi): (u32, u32)) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut i = lo;
        while i < hi {
            let e = self.close[i as usize].max(i);
            out.push((i, e));
            i = e + 1;
        }
        out
    }
}

fn push_kind(op: &SceneOp) -> Option<u8> {
    match op {
        SceneOp::PushGroup { .. } => Some(0),
        SceneOp::PushClip { .. } => Some(1),
        SceneOp::PushTransparency(_) => Some(2),
        SceneOp::PushLayer { .. } => Some(3),
        _ => None,
    }
}

fn pop_kind(op: &SceneOp) -> Option<u8> {
    match op {
        SceneOp::PopGroup => Some(0),
        SceneOp::PopClip => Some(1),
        SceneOp::PopTransparency => Some(2),
        SceneOp::PopLayer => Some(3),
        _ => None,
    }
}

/// A pair of child ranges to align, under a common transform.
struct Work {
    a: (u32, u32),
    b: (u32, u32),
    xf: Transform2D,
}

struct Diff<'a> {
    a: Side<'a>,
    b: Side<'a>,
    view: DeviceRect,
    /// Whether a ramp id resolves to the same tables on both sides.
    ramps: HashMap<RampId, bool>,
    /// Whether an image id resolves to the same pixels on both sides.
    images: HashMap<ImageId, bool>,
    rects: Vec<DeviceRect>,
    removed: usize,
    added: usize,
    same_resolver: bool,
}

impl Diff<'_> {
    /// Aligns the children of one pair of ranges, queues the matched
    /// subtrees and records the damage of the unmatched children.
    fn children(&mut self, w: &Work, work: &mut Vec<Work>) {
        let ca = self.a.children(w.a);
        let cb = self.b.children(w.b);
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut p = 0;
        while p < ca.len() && p < cb.len() && self.same(ca[p], cb[p]) {
            pairs.push((p, p));
            p += 1;
        }
        let (mut ea, mut eb) = (ca.len(), cb.len());
        while ea > p && eb > p && self.same(ca[ea - 1], cb[eb - 1]) {
            ea -= 1;
            eb -= 1;
        }
        // The middle: bucket the new side by key, take the first candidate
        // of the old child's key if it is equal. Duplicated keys are
        // matched in order; a mismatch is simply left unmatched, which only
        // ever adds damage.
        let mut middle: Vec<(usize, usize)> = Vec::new();
        if p < ea && p < eb {
            let mut buckets: HashMap<u64, std::collections::VecDeque<usize>> = HashMap::new();
            for (j, c) in cb.iter().enumerate().take(eb).skip(p) {
                buckets.entry(key(self.b.op(c.0))).or_default().push_back(j);
            }
            for (i, c) in ca.iter().enumerate().take(ea).skip(p) {
                let Some(q) = buckets.get_mut(&key(self.a.op(c.0))) else {
                    continue;
                };
                if let Some(&j) = q.front()
                    && self.same(*c, cb[j])
                {
                    q.pop_front();
                    middle.push((i, j));
                }
            }
            middle = longest_increasing(&middle);
        }
        pairs.extend(middle);
        pairs.extend((ea..ca.len()).zip(eb..cb.len()));

        let mut used_a = vec![false; ca.len()];
        let mut used_b = vec![false; cb.len()];
        for &(i, j) in &pairs {
            used_a[i] = true;
            used_b[j] = true;
            let (sa, sb) = (ca[i], cb[j]);
            if sa.0 != sa.1 {
                let xf = match self.a.op(sa.0) {
                    SceneOp::PushGroup { xf: g, .. } => g.then(w.xf),
                    _ => w.xf,
                };
                work.push(Work {
                    a: (sa.0 + 1, sa.1),
                    b: (sb.0 + 1, sb.1),
                    xf,
                });
            }
        }
        for (c, used) in ca.iter().zip(&used_a) {
            if !used {
                self.removed += leaf_bounds(&self.a, *c, w.xf, self.view, &mut self.rects);
            }
        }
        for (c, used) in cb.iter().zip(&used_b) {
            if !used {
                self.added += leaf_bounds(&self.b, *c, w.xf, self.view, &mut self.rects);
            }
        }
    }

    /// Whether two children match: a leaf draws exactly the same thing, a
    /// subtree opens with the same push (its insides are compared later).
    fn same(&mut self, a: (u32, u32), b: (u32, u32)) -> bool {
        let (oa, ob) = (self.a.op(a.0), self.b.op(b.0));
        (a.0 == a.1) == (b.0 == b.1) && oa == ob && self.same_resources(oa)
    }

    /// Whether every ramp and image `op` refers to resolves to the same
    /// data on both sides. Ids are interned per walker and an evicted ramp
    /// slot is reused, so equal ids are not enough.
    fn same_resources(&mut self, op: &SceneOp) -> bool {
        if self.same_resolver {
            return true;
        }
        let mut ok = true;
        refs(op, &mut |r| {
            ok = ok
                && match r {
                    Ref::Ramp(id) => *self.ramps.entry(id).or_insert_with(|| {
                        let (x, y) = (self.a.res, self.b.res);
                        x.ramps.try_get(id) == y.ramps.try_get(id)
                            && x.transparency_ramps.get(id.index() as usize)
                                == y.transparency_ramps.get(id.index() as usize)
                    }),
                    Ref::Image(id) => *self
                        .images
                        .entry(id)
                        .or_insert_with(|| self.a.res.images.get(id) == self.b.res.images.get(id)),
                };
        });
        ok
    }
}

/// Adds the device bounds of every leaf of `child` to `out`, clipped to
/// the viewport. Returns how many leaves there were.
fn leaf_bounds(
    side: &Side<'_>,
    child: (u32, u32),
    xf: Transform2D,
    view: DeviceRect,
    out: &mut Vec<DeviceRect>,
) -> usize {
    let mut stack = vec![xf];
    let mut xf = xf;
    let mut leaves = 0;
    for i in child.0..=child.1 {
        let b = match side.op(i) {
            SceneOp::PushGroup { xf: g, .. } => {
                xf = g.then(xf);
                stack.push(xf);
                continue;
            }
            SceneOp::PopGroup => {
                stack.pop();
                xf = stack.last().copied().unwrap_or(xf);
                continue;
            }
            SceneOp::Fill { path, .. } => device_bounds_of(path, xf, 0.0),
            SceneOp::Stroke { path, style, .. } => {
                device_bounds_of(path, xf, stroke_pad(style)).inflated(1)
            }
            SceneOp::Image { mapping, .. } => mapping_bounds(mapping.transformed(xf)),
            _ => continue,
        };
        leaves += 1;
        let b = b.intersection(view);
        if !b.is_empty() {
            out.push(b);
        }
    }
    leaves
}

/// A cheap key consistent with op equality: equal ops have equal keys.
/// Two different ops may share one; the equality test decides.
fn key(op: &SceneOp) -> u64 {
    fn mix(h: u64, v: u64) -> u64 {
        let mut z = h.rotate_left(5) ^ v.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn rect(h: u64, r: kurbo::Rect) -> u64 {
        // Equal paths have bit-identical bounds (they are computed from
        // the path), so hashing the bits is consistent with equality.
        [r.x0, r.y0, r.x1, r.y1]
            .iter()
            .fold(h, |h, v| mix(h, v.to_bits()))
    }
    match op {
        SceneOp::PushGroup { id, .. } => mix(1, id.0),
        SceneOp::PushClip { path, .. } => rect(2, path.bounds()),
        SceneOp::PushTransparency(t) => mix(3, t.family as u64),
        SceneOp::PushLayer { kind, transparency } => {
            mix(mix(4, *kind as u64), transparency.family as u64)
        }
        SceneOp::Fill { id, path, .. } => rect(mix(5, id.0), path.bounds()),
        SceneOp::Stroke { id, path, .. } => rect(mix(6, id.0), path.bounds()),
        SceneOp::Image { id, image, .. } => mix(mix(7, id.0), u64::from(image.index())),
        SceneOp::PopGroup | SceneOp::PopClip | SceneOp::PopTransparency | SceneOp::PopLayer => 8,
    }
}

/// A resource an op refers to by id.
#[derive(Debug, Clone, Copy)]
enum Ref {
    Ramp(RampId),
    Image(ImageId),
}

fn refs(op: &SceneOp, f: &mut impl FnMut(Ref)) {
    match op {
        SceneOp::Fill {
            paint,
            transparency,
            ..
        }
        | SceneOp::Stroke {
            paint,
            transparency,
            ..
        } => {
            paint_refs(paint, f);
            transparency_refs(transparency, f);
        }
        SceneOp::Image {
            image,
            paint,
            transparency,
            ..
        } => {
            f(Ref::Image(*image));
            paint_refs(paint, f);
            transparency_refs(transparency, f);
        }
        SceneOp::PushTransparency(t)
        | SceneOp::PushLayer {
            transparency: t, ..
        } => transparency_refs(t, f),
        _ => {}
    }
}

fn paint_refs(p: &Paint, f: &mut impl FnMut(Ref)) {
    match p {
        Paint::Gradient {
            ramp: GradRamp::Table(id),
            ..
        } => f(Ref::Ramp(*id)),
        Paint::Image { image, .. } => f(Ref::Image(*image)),
        _ => {}
    }
}

fn transparency_refs(t: &Transparency, f: &mut impl FnMut(Ref)) {
    match &t.source {
        TranspSource::Gradient { ramp, .. } => f(Ref::Ramp(*ramp)),
        TranspSource::Image { image, ramp, .. } => {
            f(Ref::Image(*image));
            if let Some(ramp) = ramp {
                f(Ref::Ramp(*ramp));
            }
        }
        TranspSource::Flat(_) | TranspSource::Mesh { .. } => {}
    }
}

/// The longest subsequence of `pairs` (increasing in the first index)
/// whose second index also increases.
fn longest_increasing(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // Patience sorting: `tails[k]` is the index in `pairs` of the smallest
    // tail of an increasing run of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; pairs.len()];
    for (i, &(_, j)) in pairs.iter().enumerate() {
        let k = tails.partition_point(|&t| pairs[t].1 < j);
        prev[i] = k.checked_sub(1).map(|k| tails[k]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(i) = at {
        out.push(pairs[i]);
        at = prev[i];
    }
    out.reverse();
    out
}

/// Merges `rects` into at most `max` rectangles, always merging the pair
/// whose union adds the least area, and merging below the limit too when
/// a union costs nothing (the two overlap or abut enough that their
/// bounding box is no larger than the pair).
#[must_use]
pub fn coalesce(mut rects: Vec<DeviceRect>, max: usize) -> Vec<DeviceRect> {
    rects.retain(|r| !r.is_empty());
    if rects.len() > PRE_MERGE {
        rects.sort_unstable_by_key(|r| (r.y0, r.x0));
        let per = rects.len().div_ceil(PRE_MERGE);
        rects = rects
            .chunks(per)
            .map(|c| c.iter().fold(DeviceRect::EMPTY, |a, r| a.union(*r)))
            .collect();
    }
    loop {
        let mut best: Option<(i128, usize, usize)> = None;
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (rects[i], rects[j]);
                let cost =
                    i128::from(a.union(b).area()) - i128::from(a.area()) - i128::from(b.area());
                if best.is_none_or(|(c, _, _)| cost < c) {
                    best = Some((cost, i, j));
                }
            }
        }
        match best {
            Some((cost, i, j)) if cost <= 0 || rects.len() > max => {
                let b = rects.swap_remove(j);
                rects[i] = rects[i].union(b);
            }
            _ => break,
        }
    }
    rects.sort_unstable_by_key(|r| (r.y0, r.x0));
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::PathRef;
    use crate::scene::{CacheHint, RenderQuality, SceneBuilder, SceneNodeId};
    use xarast_color::Rgba8;
    use xarast_geom::FillRule;
    use xarast_geom::{Mp, Point, Rect};

    fn square(x: f64, y: f64, s: f64) -> PathRef {
        let mut b = xarast_geom::Path::builder();
        b.rect(Rect::new(
            Point::new(Mp::from_pt(x), Mp::from_pt(y)),
            Point::new(Mp::from_pt(x + s), Mp::from_pt(y + s)),
        ));
        PathRef::new(b.build())
    }

    fn solid(r: u8) -> Paint {
        Paint::Solid(Rgba8 {
            r,
            g: 0,
            b: 0,
            a: 255,
        })
    }

    /// Document units are millipoints: one point per pixel.
    const PX: Transform2D = Transform2D::new([1e-3, 0.0, 0.0, 1e-3, 0.0, 0.0]);

    fn view() -> ViewParams {
        ViewParams {
            transform: PX,
            viewport: DeviceRect::from_size(400, 300),
            ..ViewParams::default()
        }
    }

    /// Three squares in a group; `f` may change what is recorded.
    fn scene(f: impl Fn(&mut SceneBuilder<'_>, usize, &PathRef, &Paint) -> bool) -> Scene {
        let mut s = Scene::new();
        let mut b = SceneBuilder::begin(&mut s, RenderQuality::Final);
        b.push_group(SceneNodeId(100), Transform2D::IDENTITY, CacheHint::Auto);
        let paths = [
            square(10.0, 10.0, 20.0),
            square(100.0, 10.0, 20.0),
            square(200.0, 100.0, 30.0),
        ];
        for (i, p) in paths.iter().enumerate() {
            let paint = solid(50 * (i as u8 + 1));
            if !f(&mut b, i, p, &paint) {
                b.fill(SceneNodeId(i as u64 + 1), p, FillRule::NonZero, paint);
            }
        }
        b.pop_group();
        b.finish().unwrap();
        s
    }

    fn damage(a: &Scene, b: &Scene) -> Damage {
        let r = Resolver::new();
        scene_damage((a, &r), (b, &r), &view(), 8).unwrap()
    }

    fn bounds_of(p: &PathRef) -> DeviceRect {
        device_bounds_of(p, PX, 0.0)
    }

    #[test]
    fn the_same_scene_has_no_damage() {
        let a = scene(|_, _, _, _| false);
        let b = scene(|_, _, _, _| false);
        assert_eq!(damage(&a, &b), Damage::default());
    }

    #[test]
    fn a_recoloured_object_damages_only_its_bounds() {
        let a = scene(|_, _, _, _| false);
        let b = scene(|b, i, p, _| {
            if i != 1 {
                return false;
            }
            b.fill(SceneNodeId(2), p, FillRule::NonZero, solid(7));
            true
        });
        let d = damage(&a, &b);
        assert_eq!(d.rects, vec![bounds_of(&square(100.0, 10.0, 20.0))]);
        assert_eq!((d.removed, d.added), (1, 1));
    }

    #[test]
    fn a_moved_object_damages_where_it_was_and_where_it_is() {
        let a = scene(|_, _, _, _| false);
        let moved = square(300.0, 200.0, 20.0);
        let b = scene(|b, i, _, paint| {
            if i != 0 {
                return false;
            }
            b.fill(SceneNodeId(1), &moved, FillRule::NonZero, paint.clone());
            true
        });
        let d = damage(&a, &b);
        let (was, is) = (bounds_of(&square(10.0, 10.0, 20.0)), bounds_of(&moved));
        assert_eq!(d.rects.len(), 2);
        assert!(d.rects.contains(&was) && d.rects.contains(&is), "{d:?}");
    }

    #[test]
    fn a_z_order_swap_damages_both_objects() {
        let a = scene(|_, _, _, _| false);
        // Record the last two in the other order.
        let b = scene(|b, i, p, paint| match i {
            1 => true,
            2 => {
                b.fill(SceneNodeId(3), p, FillRule::NonZero, paint.clone());
                b.fill(
                    SceneNodeId(2),
                    &square(100.0, 10.0, 20.0),
                    FillRule::NonZero,
                    solid(100),
                );
                true
            }
            _ => false,
        });
        let d = damage(&a, &b);
        // One of the two keeps its place in the order; the other moves.
        assert_eq!(d.removed + d.added, 2, "{d:?}");
    }

    #[test]
    fn a_group_transform_damages_everything_under_it() {
        let a = scene(|_, _, _, _| false);
        let mut b = Scene::new();
        {
            let mut sb = SceneBuilder::begin(&mut b, RenderQuality::Final);
            sb.push_group(
                SceneNodeId(100),
                Transform2D::new([1.0, 0.0, 0.0, 1.0, 5.0, 0.0]),
                CacheHint::Auto,
            );
            for (i, p) in [
                square(10.0, 10.0, 20.0),
                square(100.0, 10.0, 20.0),
                square(200.0, 100.0, 30.0),
            ]
            .iter()
            .enumerate()
            {
                sb.fill(
                    SceneNodeId(i as u64 + 1),
                    p,
                    FillRule::NonZero,
                    solid(50 * (i as u8 + 1)),
                );
            }
            sb.pop_group();
            sb.finish().unwrap();
        }
        let d = damage(&a, &b);
        assert_eq!((d.removed, d.added), (3, 3));
    }

    #[test]
    fn a_reused_ramp_slot_is_damage_even_with_equal_ops() {
        use crate::paint::{GradMapping, GradShape, Repeat};
        use crate::ramp::{EffectSpace, Profile, RampLength, Stop};
        let stops = |c: u8| {
            vec![
                Stop {
                    offset: 0.0,
                    color: Rgba8::BLACK,
                },
                Stop {
                    offset: 1.0,
                    color: Rgba8 {
                        r: c,
                        g: c,
                        b: c,
                        a: 255,
                    },
                },
            ]
        };
        let mut r1 = Resolver::new();
        let id = r1.ramps.intern(
            &stops(255),
            Profile::default(),
            EffectSpace::default(),
            RampLength::Short,
        );
        let mut r2 = Resolver::new();
        let id2 = r2.ramps.intern(
            &stops(9),
            Profile::default(),
            EffectSpace::default(),
            RampLength::Short,
        );
        assert_eq!(id, id2, "both caches hand out the first slot");
        let grad = Paint::Gradient {
            shape: GradShape::Linear,
            mapping: GradMapping::unit(),
            repeat: Repeat::Simple,
            ramp: GradRamp::Table(id),
        };
        let a = scene(|b, i, p, _| {
            if i != 2 {
                return false;
            }
            b.fill(SceneNodeId(3), p, FillRule::NonZero, grad.clone());
            true
        });
        let d = scene_damage((&a, &r1), (&a, &r2), &view(), 8).unwrap();
        assert_eq!(d.rects, vec![bounds_of(&square(200.0, 100.0, 30.0))]);
        let d = scene_damage((&a, &r1), (&a, &r1.clone()), &view(), 8).unwrap();
        assert!(d.rects.is_empty());
    }

    #[test]
    fn scenes_of_different_quality_are_not_compared() {
        let a = scene(|_, _, _, _| false);
        let mut b = Scene::new();
        SceneBuilder::begin(&mut b, RenderQuality::Draft)
            .finish()
            .unwrap();
        let r = Resolver::new();
        assert!(scene_damage((&a, &r), (&b, &r), &view(), 8).is_none());
    }

    #[test]
    fn the_longest_increasing_run_keeps_order() {
        let pairs = [(0, 3), (1, 0), (2, 1), (3, 4), (4, 2), (5, 5)];
        assert_eq!(
            longest_increasing(&pairs),
            vec![(1, 0), (2, 1), (4, 2), (5, 5)]
        );
        assert!(longest_increasing(&[]).is_empty());
    }

    #[test]
    fn coalescing_merges_overlaps_and_respects_the_limit() {
        let r = |x: i32, y: i32| DeviceRect::new(x, y, x + 10, y + 10);
        // Overlapping pieces merge even under the limit.
        assert_eq!(
            coalesce(vec![r(0, 0), DeviceRect::new(0, 0, 10, 5)], 8),
            vec![r(0, 0)]
        );
        // Far apart pieces stay apart under the limit...
        assert_eq!(coalesce(vec![r(0, 0), r(100, 100)], 8).len(), 2);
        // ...and are merged, cheapest first, above it.
        let out = coalesce(vec![r(0, 0), r(12, 0), r(200, 200)], 2);
        assert_eq!(out, vec![DeviceRect::new(0, 0, 22, 10), r(200, 200)]);
        // Thousands of pieces stay cheap and are covered.
        let many: Vec<DeviceRect> = (0..3000).map(|i| r(i % 300, i / 10)).collect();
        let out = coalesce(many.clone(), 4);
        assert!(out.len() <= 4);
        for m in many {
            assert!(
                out.iter().any(|o| o.intersection(m) == m) || {
                    // A piece may be split across merged rectangles only if
                    // their union covers it.
                    out.iter().fold(0, |a, o| a + o.intersection(m).area()) >= m.area()
                }
            );
        }
    }
}
