//! A spatial index over many objects' bounds, for picking and marquee
//! selection.
//!
//! # What it is for
//!
//! The selector asks two questions on every pointer event: *which objects
//! could be under this point* (then, top-down in z-order, which is the first
//! one whose real geometry is hit), and *which objects does this marquee
//! touch or enclose*. [`HitIndex`] answers the first with candidates in
//! descending z so the precise test ([`HitShape::hit`](crate::HitShape::hit))
//! can stop at the first hit, and the second with the final answer on
//! bounds.
//!
//! # The structure: a hashed uniform grid
//!
//! Chosen over a static BVH by benchmark at 100 000 objects; the numbers
//! and the reasoning are in `docs/memory/geometry.md`. In short: queries
//! were comparable, and the grid's edits are O(cells an object covers)
//! where a BVH has to be refitted or rebuilt.
//!
//! - Cells are squares of side `cell_size` millipoints, keyed by their
//!   integer coordinates in a hash map, so only occupied cells cost memory
//!   and a far-away object does not stretch a dense array.
//! - An object is listed in every cell its bounds overlap. One that would
//!   overlap more than [`MAX_CELLS_PER_ENTRY`] cells — a page background, a
//!   frame round everything — goes on a separate **large** list that every
//!   query scans instead, so no edit or query ever touches thousands of
//!   cells for one object.
//! - `cell_size` is re-derived from the objects themselves (twice the median
//!   object extent, with a floor from the overall density) whenever the
//!   population has doubled or quartered since it was last chosen, so the
//!   cost of retuning is amortised O(1) per edit.
//! - A query that would visit more cells than are occupied walks the
//!   occupied cells instead, so a marquee over the whole document costs
//!   O(objects), never O(area).
//!
//! # Z-order
//!
//! Every entry carries a `u64` z supplied by the caller: larger is nearer
//! the viewer. The index never compares keys, so z is the only order it
//! knows; two entries with the same z come out in an unspecified (but
//! deterministic) order. See the integration contract in
//! `docs/memory/geometry.md` for how the app assigns it.

use crate::{Mp, Point, Rect};
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hash, Hasher};

/// An entry overlapping more cells than this goes on the large list.
pub const MAX_CELLS_PER_ENTRY: u64 = 16;

/// The cell size an empty index starts with: one inch.
const INITIAL_CELL: i64 = 72_000;

/// Smallest and largest cell sizes, in millipoints. The upper bound keeps
/// cell coordinates comfortably inside `i32`.
const MIN_CELL: i64 = 64;
const MAX_CELL: i64 = 1 << 28;

/// A multiplicative hasher for small integer keys (cell coordinates, slot
/// numbers, slotmap keys). Not DoS-resistant, which does not matter for an
/// index whose keys the application allocates.
#[derive(Default, Clone, Copy, Debug)]
struct FastHasher(u64);

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

/// How a marquee selects.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum RectMode {
    /// Everything whose bounds intersect the rectangle, boundary included.
    Touch,
    /// Everything whose bounds lie entirely inside the rectangle.
    Enclose,
}

/// A cell range, inclusive at both ends.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct CellRange {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl CellRange {
    fn count(self) -> u64 {
        let w = u64::try_from(i64::from(self.x1) - i64::from(self.x0) + 1).unwrap_or(0);
        let h = u64::try_from(i64::from(self.y1) - i64::from(self.y0) + 1).unwrap_or(0);
        w.saturating_mul(h)
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

/// Where an entry is listed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Place {
    /// A free slot.
    Free,
    /// Live, but with empty bounds: it can never be hit.
    Nowhere,
    /// In every cell of the range.
    Cells(CellRange),
    /// On the large list, at this position.
    Large(u32),
}

/// One object's listing in one cell. The bounds are copied in so a scan
/// reads one contiguous array instead of chasing the slot for each entry.
#[derive(Copy, Clone, Debug)]
struct Member {
    slot: u32,
    bounds: Rect,
}

fn cell_key(x: i32, y: i32) -> u64 {
    (u64::from(x as u32) << 32) | u64::from(y as u32)
}

/// Counters describing the index's current shape, for benchmarks and
/// diagnostics.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct HitIndexStats {
    /// Live entries.
    pub entries: usize,
    /// Occupied cells.
    pub cells: usize,
    /// Entries on the large list.
    pub large: usize,
    /// Total cell memberships, a measure of memory.
    pub memberships: usize,
    /// The cell side, in millipoints.
    pub cell_size: i64,
}

/// A spatial index over objects' bounds. See the module documentation.
///
/// `K` is the caller's object identifier — a document `NodeId`, say. It must
/// be cheap to copy and hash.
#[derive(Clone, Debug)]
pub struct HitIndex<K> {
    keys: Vec<Option<K>>,
    bounds: Vec<Rect>,
    z: Vec<u64>,
    place: Vec<Place>,
    /// For a slot listed in cells, its index in each cell's list, in
    /// row-major order over its range, so removal and in-place updates are
    /// O(1) per cell however crowded the cell is.
    pos: Vec<[u32; MAX_CELLS_PER_ENTRY as usize]>,
    free: Vec<u32>,
    slot_of: FastMap<K, u32>,
    cells: FastMap<u64, Vec<Member>>,
    large: Vec<u32>,
    cell: i64,
    /// Live entries when `cell` was last chosen.
    tuned_for: usize,
}

impl<K: Copy + Eq + Hash> Default for HitIndex<K> {
    fn default() -> HitIndex<K> {
        HitIndex::new()
    }
}

impl<K: Copy + Eq + Hash> HitIndex<K> {
    /// An empty index.
    #[must_use]
    pub fn new() -> HitIndex<K> {
        HitIndex {
            keys: Vec::new(),
            bounds: Vec::new(),
            z: Vec::new(),
            place: Vec::new(),
            pos: Vec::new(),
            free: Vec::new(),
            slot_of: FastMap::default(),
            cells: FastMap::default(),
            large: Vec::new(),
            cell: INITIAL_CELL,
            tuned_for: 0,
        }
    }

    /// Builds an index from `(key, bounds, z)` triples in one pass, choosing
    /// the cell size for the whole population. A repeated key keeps its last
    /// triple.
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = (K, Rect, u64)>) -> HitIndex<K> {
        let mut idx = HitIndex::new();
        for (k, b, z) in entries {
            if let Some(&s) = idx.slot_of.get(&k) {
                idx.bounds[s as usize] = b;
                idx.z[s as usize] = z;
            } else {
                let s = idx.keys.len() as u32;
                idx.keys.push(Some(k));
                idx.bounds.push(b);
                idx.z.push(z);
                idx.place.push(Place::Nowhere);
                idx.pos.push([0; MAX_CELLS_PER_ENTRY as usize]);
                idx.slot_of.insert(k, s);
            }
        }
        idx.rebuild();
        idx
    }

    /// How many objects are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slot_of.len()
    }

    /// Whether nothing is indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slot_of.is_empty()
    }

    /// Whether `key` is indexed.
    #[must_use]
    pub fn contains(&self, key: K) -> bool {
        self.slot_of.contains_key(&key)
    }

    /// The bounds and z an object is indexed with.
    #[must_use]
    pub fn get(&self, key: K) -> Option<(Rect, u64)> {
        let s = *self.slot_of.get(&key)? as usize;
        Some((self.bounds[s], self.z[s]))
    }

    /// Every indexed object with its bounds and z, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (K, Rect, u64)> + '_ {
        self.keys
            .iter()
            .enumerate()
            .filter_map(|(s, k)| k.map(|k| (k, self.bounds[s], self.z[s])))
    }

    /// Adds an object, or replaces its bounds and z if it is already there,
    /// returning what it replaced. Cost: the cells its bounds cover, at most
    /// [`MAX_CELLS_PER_ENTRY`].
    pub fn insert(&mut self, key: K, bounds: Rect, z: u64) -> Option<(Rect, u64)> {
        if let Some(&s) = self.slot_of.get(&key) {
            let old = (self.bounds[s as usize], self.z[s as usize]);
            self.z[s as usize] = z;
            self.move_slot(s, bounds);
            return Some(old);
        }
        let s = if let Some(s) = self.free.pop() {
            self.keys[s as usize] = Some(key);
            self.bounds[s as usize] = bounds;
            self.z[s as usize] = z;
            s
        } else {
            let s = u32::try_from(self.keys.len()).expect("fewer than 2^32 objects");
            self.keys.push(Some(key));
            self.bounds.push(bounds);
            self.z.push(z);
            self.place.push(Place::Nowhere);
            self.pos.push([0; MAX_CELLS_PER_ENTRY as usize]);
            s
        };
        self.slot_of.insert(key, s);
        self.place_slot(s);
        if self.len() > 2 * self.tuned_for + 64 {
            self.rebuild();
        }
        None
    }

    /// Moves or resizes an object. Returns `false` if it is not indexed.
    /// When the new bounds cover the same cells as the old — a nudge within
    /// a cell — this is O(1).
    pub fn set_bounds(&mut self, key: K, bounds: Rect) -> bool {
        let Some(&s) = self.slot_of.get(&key) else {
            return false;
        };
        self.move_slot(s, bounds);
        true
    }

    /// Changes an object's z. O(1). Returns `false` if it is not indexed.
    pub fn set_z(&mut self, key: K, z: u64) -> bool {
        let Some(&s) = self.slot_of.get(&key) else {
            return false;
        };
        self.z[s as usize] = z;
        true
    }

    /// Removes an object, returning its bounds and z.
    pub fn remove(&mut self, key: K) -> Option<(Rect, u64)> {
        let s = self.slot_of.remove(&key)?;
        let old = (self.bounds[s as usize], self.z[s as usize]);
        self.unplace_slot(s);
        self.keys[s as usize] = None;
        self.place[s as usize] = Place::Free;
        self.bounds[s as usize] = Rect::EMPTY;
        self.free.push(s);
        if self.tuned_for > 256 && self.len() < self.tuned_for / 4 {
            self.rebuild();
        }
        Some(old)
    }

    /// Keeps only the objects for which `f(key, bounds, z)` is true: the
    /// bulk removal for hiding or locking a layer.
    pub fn retain(&mut self, mut f: impl FnMut(K, Rect, u64) -> bool) {
        let doomed: Vec<K> = self
            .iter()
            .filter(|&(k, b, z)| !f(k, b, z))
            .map(|(k, _, _)| k)
            .collect();
        for k in doomed {
            self.remove(k);
        }
    }

    /// Removes everything.
    pub fn clear(&mut self) {
        *self = HitIndex::new();
    }

    /// Re-chooses the cell size for the current population and re-lists
    /// every object. O(n); done automatically when the population doubles
    /// or quarters, so a caller need not, but it may after a bulk edit.
    pub fn rebuild(&mut self) {
        self.cell = self.choose_cell();
        self.cells = FastMap::default();
        self.large.clear();
        let mut keys = Vec::new();
        let mut bounds = Vec::new();
        let mut z = Vec::new();
        let mut place = Vec::new();
        // Compact: drop free slots so queries touch no holes.
        self.slot_of.clear();
        for s in 0..self.keys.len() {
            if let Some(k) = self.keys[s] {
                let n = keys.len() as u32;
                keys.push(Some(k));
                bounds.push(self.bounds[s]);
                z.push(self.z[s]);
                place.push(Place::Nowhere);
                self.slot_of.insert(k, n);
            }
        }
        self.keys = keys;
        self.bounds = bounds;
        self.z = z;
        self.pos = vec![[0; MAX_CELLS_PER_ENTRY as usize]; place.len()];
        self.place = place;
        self.free = Vec::new();
        for s in 0..self.keys.len() as u32 {
            self.place_slot(s);
        }
        self.tuned_for = self.len();
    }

    /// Counters describing the index.
    #[must_use]
    pub fn stats(&self) -> HitIndexStats {
        HitIndexStats {
            entries: self.len(),
            cells: self.cells.len(),
            large: self.large.len(),
            memberships: self.cells.values().map(Vec::len).sum(),
            cell_size: self.cell,
        }
    }

    /// Candidates for a pick at `p` with radius `radius`: every object whose
    /// bounds come within `radius` of `p`, **nearest the viewer first**.
    ///
    /// The iterator is lazy past the first step: collecting the candidates
    /// is O(candidates), and each one taken costs O(log candidates), so
    /// stopping at the first precise hit is cheap even under a thousand
    /// overlapping objects.
    #[must_use]
    pub fn candidates_at(&self, p: Point, radius: Mp) -> Candidates<'_, K> {
        let r = radius.abs();
        let q = Rect::new(Point::new(p.x - r, p.y - r), Point::new(p.x + r, p.y + r));
        let r2 = r.to_f64() * r.to_f64();
        let mut found: Vec<(u64, u32)> = Vec::new();
        self.visit(q, |s, b| {
            if rect_distance_sq(p, b) <= r2 {
                found.push((self.z[s as usize], s));
            }
        });
        Candidates {
            index: self,
            heap: BinaryHeap::from(found),
        }
    }

    /// The topmost object near `p` for which `accept` — the precise test —
    /// returns true.
    pub fn topmost(&self, p: Point, radius: Mp, mut accept: impl FnMut(K) -> bool) -> Option<K> {
        self.candidates_at(p, radius)
            .find(|&(k, _)| accept(k))
            .map(|(k, _)| k)
    }

    /// Appends every object selected by a marquee to `out`, in no particular
    /// order.
    pub fn query_rect(&self, rect: Rect, mode: RectMode, out: &mut Vec<K>) {
        if rect.is_empty() {
            return;
        }
        self.visit(rect, |s, b| {
            let hit = match mode {
                RectMode::Touch => rect.intersects(b),
                RectMode::Enclose => rect.contains_rect(b),
            };
            if hit && let Some(k) = self.keys[s as usize] {
                out.push(k);
            }
        });
    }

    fn cell_of(&self, x: Mp, y: Mp) -> (i32, i32) {
        let cx = i64::from(x.raw()).div_euclid(self.cell);
        let cy = i64::from(y.raw()).div_euclid(self.cell);
        // |raw| < 2^31 and cell >= 1, so both fit.
        (cx as i32, cy as i32)
    }

    fn range_of(&self, r: Rect) -> CellRange {
        let (x0, y0) = self.cell_of(r.lo.x, r.lo.y);
        let (x1, y1) = self.cell_of(r.hi.x, r.hi.y);
        CellRange { x0, y0, x1, y1 }
    }

    /// Calls `f(slot, bounds)` once for every slot whose cells overlap
    /// `q`'s, plus every large entry. `f` still has to test the bounds.
    fn visit(&self, q: Rect, mut f: impl FnMut(u32, Rect)) {
        if q.is_empty() {
            return;
        }
        for &s in &self.large {
            f(s, self.bounds[s as usize]);
        }
        let qr = self.range_of(q);
        let single = qr.x0 == qr.x1 && qr.y0 == qr.y1;
        // Report a slot only from the first cell where its range and the
        // query's overlap, so a multi-cell entry is seen once, with no
        // per-query bookkeeping. An entry listed in cell (cx, cy) starts at
        // or before it, so "first" means: cx is the query's first column or
        // the entry's own, and likewise for rows.
        let cell = self.cell;
        let first = |v: i32, lo: Mp| i64::from(lo.raw()).div_euclid(cell) == i64::from(v);
        let mut emit = |cx: i32, cy: i32, list: &Vec<Member>| {
            for m in list {
                if single
                    || ((cx == qr.x0 || first(cx, m.bounds.lo.x))
                        && (cy == qr.y0 || first(cy, m.bounds.lo.y)))
                {
                    f(m.slot, m.bounds);
                }
            }
        };
        if qr.count() <= self.cells.len() as u64 {
            for cx in qr.x0..=qr.x1 {
                for cy in qr.y0..=qr.y1 {
                    if let Some(list) = self.cells.get(&cell_key(cx, cy)) {
                        emit(cx, cy, list);
                    }
                }
            }
        } else {
            for (&key, list) in &self.cells {
                let (cx, cy) = ((key >> 32) as u32 as i32, key as u32 as i32);
                if qr.contains(cx, cy) {
                    emit(cx, cy, list);
                }
            }
        }
    }

    fn move_slot(&mut self, s: u32, bounds: Rect) {
        let new_place = self.place_for(bounds);
        let old_place = self.place[s as usize];
        self.bounds[s as usize] = bounds;
        let same = match (old_place, new_place) {
            (Place::Cells(a), Place::Cells(b)) => a == b,
            (Place::Large(_), Place::Large(_)) | (Place::Nowhere, Place::Nowhere) => true,
            _ => false,
        };
        if !same {
            self.unplace_slot(s);
            self.place_slot(s);
        } else if let Place::Cells(r) = old_place {
            // Same cells: refresh the copies of the bounds in place.
            let pos = self.pos[s as usize];
            for (k, (cx, cy)) in cells_of(r).enumerate() {
                if let Some(list) = self.cells.get_mut(&cell_key(cx, cy)) {
                    list[pos[k] as usize].bounds = bounds;
                }
            }
        }
    }

    /// Where bounds would be listed; `Large(0)` stands for "large".
    fn place_for(&self, b: Rect) -> Place {
        if b.is_empty() {
            return Place::Nowhere;
        }
        let r = self.range_of(b);
        if r.count() > MAX_CELLS_PER_ENTRY {
            Place::Large(0)
        } else {
            Place::Cells(r)
        }
    }

    fn place_slot(&mut self, s: u32) {
        let bounds = self.bounds[s as usize];
        let p = match self.place_for(bounds) {
            Place::Cells(r) => {
                for (k, (cx, cy)) in cells_of(r).enumerate() {
                    let list = self.cells.entry(cell_key(cx, cy)).or_default();
                    self.pos[s as usize][k] = list.len() as u32;
                    list.push(Member { slot: s, bounds });
                }
                Place::Cells(r)
            }
            Place::Large(_) => {
                self.large.push(s);
                Place::Large((self.large.len() - 1) as u32)
            }
            other => other,
        };
        self.place[s as usize] = p;
    }

    fn unplace_slot(&mut self, s: u32) {
        match self.place[s as usize] {
            Place::Cells(r) => {
                let pos = self.pos[s as usize];
                for (k, (cx, cy)) in cells_of(r).enumerate() {
                    let key = cell_key(cx, cy);
                    let Some(list) = self.cells.get_mut(&key) else {
                        continue;
                    };
                    let i = pos[k] as usize;
                    list.swap_remove(i);
                    if let Some(moved) = list.get(i).copied() {
                        // The member now at `i` came from the end: record
                        // its new index under this cell's position in its
                        // own range.
                        if let Place::Cells(mr) = self.place[moved.slot as usize] {
                            let h = i64::from(mr.y1) - i64::from(mr.y0) + 1;
                            let j = (i64::from(cx) - i64::from(mr.x0)) * h
                                + (i64::from(cy) - i64::from(mr.y0));
                            self.pos[moved.slot as usize][j as usize] = i as u32;
                        }
                    }
                    if list.is_empty() {
                        self.cells.remove(&key);
                    }
                }
            }
            Place::Large(i) => {
                let i = i as usize;
                self.large.swap_remove(i);
                if let Some(&moved) = self.large.get(i) {
                    self.place[moved as usize] = Place::Large(i as u32);
                }
            }
            Place::Nowhere | Place::Free => {}
        }
        self.place[s as usize] = Place::Nowhere;
    }

    /// Twice the median object extent, but no finer than half the mean
    /// spacing the population would have if spread evenly over its union,
    /// so a document of dots does not become millions of empty-ish cells.
    fn choose_cell(&self) -> i64 {
        let mut extents: Vec<i64> = Vec::new();
        let mut union = Rect::EMPTY;
        for (s, k) in self.keys.iter().enumerate() {
            let b = self.bounds[s];
            if k.is_some() && !b.is_empty() {
                extents.push(i64::from(b.width().raw()).max(i64::from(b.height().raw())));
                union = union.union(b);
            }
        }
        if extents.is_empty() {
            return INITIAL_CELL;
        }
        let mid = extents.len() / 2;
        let median = *extents.select_nth_unstable(mid).1;
        let area = union.width().to_f64() * union.height().to_f64();
        let spacing = (area / extents.len() as f64).sqrt() * 0.5;
        let spacing = if spacing.is_finite() {
            spacing as i64
        } else {
            0
        };
        (2 * median).max(spacing).clamp(MIN_CELL, MAX_CELL)
    }
}

/// The cells of a range in row-major order: x outer, y inner. The order
/// `pos` is indexed in.
fn cells_of(r: CellRange) -> impl Iterator<Item = (i32, i32)> {
    (r.x0..=r.x1).flat_map(move |cx| (r.y0..=r.y1).map(move |cy| (cx, cy)))
}

/// Squared distance from a point to a rectangle, zero inside.
fn rect_distance_sq(p: Point, r: Rect) -> f64 {
    let (px, py) = p.to_f64();
    let dx = (r.lo.x.to_f64() - px).max(px - r.hi.x.to_f64()).max(0.0);
    let dy = (r.lo.y.to_f64() - py).max(py - r.hi.y.to_f64()).max(0.0);
    dx * dx + dy * dy
}

/// Pick candidates in descending z, from [`HitIndex::candidates_at`].
#[derive(Debug)]
pub struct Candidates<'a, K> {
    index: &'a HitIndex<K>,
    heap: BinaryHeap<(u64, u32)>,
}

impl<K: Copy> Iterator for Candidates<'_, K> {
    /// The object and its z.
    type Item = (K, u64);

    fn next(&mut self) -> Option<(K, u64)> {
        let (z, s) = self.heap.pop()?;
        self.index.keys[s as usize].map(|k| (k, z))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.heap.len(), Some(self.heap.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_in_descending_z() {
        let mut idx = HitIndex::new();
        idx.insert(1u32, Rect::raw(0, 0, 100, 100), 10);
        idx.insert(2, Rect::raw(50, 50, 150, 150), 20);
        idx.insert(3, Rect::raw(500, 500, 600, 600), 30);
        let got: Vec<_> = idx.candidates_at(Point::raw(75, 75), Mp::ZERO).collect();
        assert_eq!(got, vec![(2, 20), (1, 10)]);
        assert_eq!(
            idx.topmost(Point::raw(75, 75), Mp::ZERO, |k| k != 2),
            Some(1)
        );
        // A radius reaches the third from outside its bounds.
        assert_eq!(
            idx.topmost(Point::raw(450, 450), Mp::new(71), |_| true),
            Some(3)
        );
        assert_eq!(
            idx.topmost(Point::raw(450, 450), Mp::new(70), |_| true),
            None
        );
    }

    #[test]
    fn marquee_touch_and_enclose() {
        let mut idx = HitIndex::new();
        idx.insert('a', Rect::raw(0, 0, 100, 100), 1);
        idx.insert('b', Rect::raw(90, 90, 300, 300), 2);
        let mut out = Vec::new();
        idx.query_rect(Rect::raw(-10, -10, 150, 150), RectMode::Enclose, &mut out);
        assert_eq!(out, vec!['a']);
        out.clear();
        idx.query_rect(Rect::raw(-10, -10, 150, 150), RectMode::Touch, &mut out);
        out.sort_unstable();
        assert_eq!(out, vec!['a', 'b']);
    }

    #[test]
    fn edits_are_seen() {
        let mut idx = HitIndex::new();
        idx.insert(1u32, Rect::raw(0, 0, 10, 10), 1);
        assert!(idx.set_bounds(1, Rect::raw(1_000_000, 0, 1_000_010, 10)));
        assert!(
            idx.candidates_at(Point::raw(5, 5), Mp::ZERO)
                .next()
                .is_none()
        );
        assert_eq!(
            idx.topmost(Point::raw(1_000_005, 5), Mp::ZERO, |_| true),
            Some(1)
        );
        // Large: a background spanning far more than 16 cells.
        idx.insert(
            2,
            Rect::raw(-10_000_000, -10_000_000, 10_000_000, 10_000_000),
            0,
        );
        assert_eq!(idx.stats().large, 1);
        assert_eq!(
            idx.candidates_at(Point::raw(1_000_005, 5), Mp::ZERO)
                .count(),
            2
        );
        assert_eq!(idx.remove(2).map(|(_, z)| z), Some(0));
        assert_eq!(idx.stats().large, 0);
        assert!(!idx.set_z(2, 5));
        assert_eq!(idx.len(), 1);
    }
}
