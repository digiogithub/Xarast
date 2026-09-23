//! Uniform grid versus static BVH for the object hit index, at 100 000
//! objects, and the phase-7 picking budgets.
//!
//! The grid is the shipped [`HitIndex`]. The BVH is written here, in the
//! benchmark, so the losing candidate does not become dead code in the
//! crate: a median-split BVH with four objects per leaf, a max-z per node
//! for best-first (topmost-first) picking, contiguous subtree ranges so an
//! enclosed node emits a slice, and O(log n) refitting for moves. Inserting
//! or removing an object rebuilds it, which is the honest cost of a static
//! BVH.
//!
//! Scenes (all on an A3 page, 842 x 1191 pt):
//!
//! - `uniform`: objects of 2-40 pt scattered evenly.
//! - `clustered`: the same objects in 100 tight clusters.
//! - `mixed`: `uniform` plus 1 % of objects spanning a fifth of the page or
//!   more, the page backgrounds and frames real documents have.
//! - `stacked`: every object overlapping the page centre — the adversarial
//!   "dense overlap" case of the phase document.
//!
//! Budgets (`docs/phases/phase-07-tools-and-editing.md`): hit test on a
//! 100k-object document ≤ 2 ms worst case, marquee over 100k ≤ 20 ms. The
//! assignment for XARA-US-0031 also names 1 ms and 5 ms; both are reported
//! in `docs/memory/perf.md`.
//!
//! Run on the reference machine with
//! `taskset -c 4-7 cargo bench -p xarast-geom --bench hit_index`.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::collections::BinaryHeap;
use std::hint::black_box;
use xarast_geom::{
    FillRule, HitIndex, HitShape, HitTolerance, Matrix, Mp, Path, Point, Rect, RectMode,
    StrokeStyle,
};

const N: usize = 100_000;
const PAGE_W: i32 = 842_000;
const PAGE_H: i32 = 1_191_000;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo) as u64) as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scene {
    Uniform,
    Clustered,
    Mixed,
    Stacked,
}

/// The objects' own rectangles (their geometry is an ellipse or a
/// rectangle inscribed in it).
fn scene(kind: Scene, n: usize) -> Vec<Rect> {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let centres: Vec<(i32, i32)> = (0..100)
        .map(|_| (rng.range(0, PAGE_W), rng.range(0, PAGE_H)))
        .collect();
    (0..n)
        .map(|i| {
            let w = rng.range(2_000, 40_000);
            let h = rng.range(2_000, 40_000);
            let (x, y) = match kind {
                Scene::Uniform => (rng.range(0, PAGE_W), rng.range(0, PAGE_H)),
                Scene::Clustered => {
                    let (cx, cy) = centres[i % centres.len()];
                    (
                        cx + rng.range(-20_000, 20_000),
                        cy + rng.range(-20_000, 20_000),
                    )
                }
                Scene::Mixed => {
                    if i % 100 == 0 {
                        let w = rng.range(PAGE_W / 5, PAGE_W);
                        let h = rng.range(PAGE_H / 5, PAGE_H);
                        let x = rng.range(0, PAGE_W - w + 1);
                        let y = rng.range(0, PAGE_H - h + 1);
                        return Rect::raw(x, y, x + w, y + h);
                    }
                    (rng.range(0, PAGE_W), rng.range(0, PAGE_H))
                }
                Scene::Stacked => {
                    // At least 10 pt from the centre to every edge, so the
                    // hollow worst case below really is hollow.
                    let (w, h) = (w.max(10_000), h.max(10_000));
                    let (cx, cy) = (PAGE_W / 2, PAGE_H / 2);
                    return Rect::raw(cx - w, cy - h, cx + w, cy + h);
                }
            };
            Rect::raw(x - w / 2, y - h / 2, x + w / 2, y + h / 2)
        })
        .collect()
}

fn query_points(n: usize) -> Vec<Point> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    (0..n)
        .map(|_| Point::raw(rng.range(0, PAGE_W), rng.range(0, PAGE_H)))
        .collect()
}

// ───────────────────────────────────────────── the BVH competitor

#[derive(Clone, Copy, Debug)]
struct Node {
    bb: Rect,
    max_z: u64,
    /// Range of `order` this subtree covers.
    first: u32,
    count: u32,
    /// Right child; `u32::MAX` for a leaf. The left child is `self + 1`.
    right: u32,
    parent: u32,
}

struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
    bounds: Vec<Rect>,
    z: Vec<u64>,
    leaf_of: Vec<u32>,
}

const LEAF: usize = 4;

impl Bvh {
    fn build(bounds: &[Rect], z: &[u64]) -> Bvh {
        let mut order: Vec<u32> = (0..bounds.len() as u32).collect();
        let mut nodes = Vec::with_capacity(2 * bounds.len() / LEAF + 1);
        let mut leaf_of = vec![0u32; bounds.len()];
        Bvh::split(
            bounds,
            z,
            &mut order,
            0,
            bounds.len(),
            u32::MAX,
            &mut nodes,
            &mut leaf_of,
        );
        Bvh {
            nodes,
            order,
            bounds: bounds.to_vec(),
            z: z.to_vec(),
            leaf_of,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn split(
        bounds: &[Rect],
        z: &[u64],
        order: &mut [u32],
        first: usize,
        end: usize,
        parent: u32,
        nodes: &mut Vec<Node>,
        leaf_of: &mut [u32],
    ) -> u32 {
        let me = nodes.len() as u32;
        let slice = &mut order[first..end];
        let mut bb = Rect::EMPTY;
        let mut max_z = 0;
        let mut cb = Rect::EMPTY;
        for &i in slice.iter() {
            bb = bb.union(bounds[i as usize]);
            max_z = max_z.max(z[i as usize]);
            cb = cb.union_point(bounds[i as usize].centre());
        }
        nodes.push(Node {
            bb,
            max_z,
            first: first as u32,
            count: (end - first) as u32,
            right: u32::MAX,
            parent,
        });
        if end - first <= LEAF {
            for &i in slice.iter() {
                leaf_of[i as usize] = me;
            }
            return me;
        }
        let mid = (end - first) / 2;
        if cb.width() >= cb.height() {
            slice.select_nth_unstable_by_key(mid, |&i| bounds[i as usize].centre().x);
        } else {
            slice.select_nth_unstable_by_key(mid, |&i| bounds[i as usize].centre().y);
        }
        Bvh::split(bounds, z, order, first, first + mid, me, nodes, leaf_of);
        let r = Bvh::split(bounds, z, order, first + mid, end, me, nodes, leaf_of);
        nodes[me as usize].right = r;
        me
    }

    fn topmost(&self, p: Point, radius: Mp, mut accept: impl FnMut(u32) -> bool) -> Option<u32> {
        let r2 = radius.to_f64() * radius.to_f64();
        // (z, is_item, index): items and nodes share one heap, so items pop
        // in descending z interleaved with the subtrees that might beat them.
        let mut heap: BinaryHeap<(u64, bool, u32)> = BinaryHeap::new();
        if !self.nodes.is_empty() && dist_sq(p, self.nodes[0].bb) <= r2 {
            heap.push((self.nodes[0].max_z, false, 0));
        }
        while let Some((_, item, i)) = heap.pop() {
            if item {
                if accept(i) {
                    return Some(i);
                }
                continue;
            }
            let n = self.nodes[i as usize];
            if n.right == u32::MAX {
                for &o in &self.order[n.first as usize..(n.first + n.count) as usize] {
                    if dist_sq(p, self.bounds[o as usize]) <= r2 {
                        heap.push((self.z[o as usize], true, o));
                    }
                }
            } else {
                for c in [i + 1, n.right] {
                    let cn = self.nodes[c as usize];
                    if dist_sq(p, cn.bb) <= r2 {
                        heap.push((cn.max_z, false, c));
                    }
                }
            }
        }
        None
    }

    fn query_rect(&self, q: Rect, mode: RectMode, out: &mut Vec<u32>) {
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let n = self.nodes[i as usize];
            if !q.intersects(n.bb) {
                continue;
            }
            let range = &self.order[n.first as usize..(n.first + n.count) as usize];
            if q.contains_rect(n.bb) {
                out.extend_from_slice(range);
                continue;
            }
            if n.right == u32::MAX {
                for &o in range {
                    let b = self.bounds[o as usize];
                    let hit = match mode {
                        RectMode::Touch => q.intersects(b),
                        RectMode::Enclose => q.contains_rect(b),
                    };
                    if hit {
                        out.push(o);
                    }
                }
            } else {
                stack.push(i + 1);
                stack.push(n.right);
            }
        }
    }

    /// Moves one object and refits its ancestors.
    fn set_bounds(&mut self, item: u32, b: Rect) {
        self.bounds[item as usize] = b;
        let mut i = self.leaf_of[item as usize];
        loop {
            let n = self.nodes[i as usize];
            let bb = if n.right == u32::MAX {
                self.order[n.first as usize..(n.first + n.count) as usize]
                    .iter()
                    .fold(Rect::EMPTY, |a, &o| a.union(self.bounds[o as usize]))
            } else {
                self.nodes[i as usize + 1]
                    .bb
                    .union(self.nodes[n.right as usize].bb)
            };
            self.nodes[i as usize].bb = bb;
            if n.parent == u32::MAX {
                break;
            }
            i = n.parent;
        }
    }
}

fn dist_sq(p: Point, r: Rect) -> f64 {
    let (px, py) = p.to_f64();
    let dx = (r.lo.x.to_f64() - px).max(px - r.hi.x.to_f64()).max(0.0);
    let dy = (r.lo.y.to_f64() - py).max(py - r.hi.y.to_f64()).max(0.0);
    dx * dx + dy * dy
}

// ───────────────────────────────────────────── the benchmarks

fn index_of(rects: &[Rect]) -> HitIndex<u32> {
    HitIndex::from_entries(
        rects
            .iter()
            .enumerate()
            .map(|(i, &r)| (i as u32, r, i as u64)),
    )
}

fn zs(n: usize) -> Vec<u64> {
    (0..n as u64).collect()
}

fn bench_structures(c: &mut Criterion) {
    let pts = query_points(1024);
    for kind in [
        Scene::Uniform,
        Scene::Clustered,
        Scene::Mixed,
        Scene::Stacked,
    ] {
        let rects = scene(kind, N);
        let z = zs(N);
        let grid = index_of(&rects);
        let bvh = Bvh::build(&rects, &z);
        eprintln!("{kind:?}: {:?}", grid.stats());
        // The two structures must agree before their speed means anything.
        for &p in &pts[..64] {
            let r = Mp::new(3_000);
            assert_eq!(grid.topmost(p, r, |_| true), bvh.topmost(p, r, |_| true));
        }
        for mode in [RectMode::Touch, RectMode::Enclose] {
            let q = Rect::raw(0, 0, PAGE_W / 2, PAGE_H / 2);
            let (mut a, mut b) = (Vec::new(), Vec::new());
            grid.query_rect(q, mode, &mut a);
            bvh.query_rect(q, mode, &mut b);
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, b, "{kind:?} {mode:?}");
        }
        let mut g = c.benchmark_group(format!("hit_index/{kind:?}"));
        g.sample_size(20);
        g.bench_function("build/grid", |b| b.iter(|| black_box(index_of(&rects))));
        g.bench_function("build/bvh", |b| {
            b.iter(|| black_box(Bvh::build(&rects, &z)))
        });

        // Topmost by bounds alone: the index's own share of a pick.
        let radius = Mp::new(3_000);
        let mut i = 0usize;
        g.bench_function("pick/grid", |b| {
            b.iter(|| {
                i = (i + 1) & 1023;
                black_box(grid.topmost(pts[i], radius, |_| true))
            })
        });
        let mut i = 0usize;
        g.bench_function("pick/bvh", |b| {
            b.iter(|| {
                i = (i + 1) & 1023;
                black_box(bvh.topmost(pts[i], radius, |_| true))
            })
        });

        for (name, q) in [
            ("small", Rect::raw(100_000, 100_000, 150_000, 160_000)),
            ("quarter", Rect::raw(0, 0, PAGE_W / 2, PAGE_H / 2)),
            ("page", Rect::raw(-1, -1, PAGE_W + 1, PAGE_H + 1)),
        ] {
            for mode in [RectMode::Touch, RectMode::Enclose] {
                let mut out = Vec::with_capacity(N);
                g.bench_function(format!("marquee_{name}_{mode:?}/grid"), |b| {
                    b.iter(|| {
                        out.clear();
                        grid.query_rect(q, mode, &mut out);
                        black_box(out.len())
                    })
                });
                g.bench_function(format!("marquee_{name}_{mode:?}/bvh"), |b| {
                    b.iter(|| {
                        out.clear();
                        bvh.query_rect(q, mode, &mut out);
                        black_box(out.len())
                    })
                });
            }
        }

        // Edits: nudge one object by a millimetre, and move one across the
        // page. The BVH refits; the grid relists when the cells change.
        let nudge = |r: Rect, k: i32| r.translated(xarast_geom::Vector::raw(k, 0));
        let mut grid_m = grid.clone();
        let mut k = 0u32;
        g.bench_function("move_nudge/grid", |b| {
            b.iter(|| {
                k = (k + 7919) % N as u32;
                let r = rects[k as usize];
                grid_m.set_bounds(k, nudge(r, 2_835));
                grid_m.set_bounds(k, r);
            })
        });
        let mut bvh_m = Bvh::build(&rects, &z);
        g.bench_function("move_nudge/bvh", |b| {
            b.iter(|| {
                k = (k + 7919) % N as u32;
                let r = rects[k as usize];
                bvh_m.set_bounds(k, nudge(r, 2_835));
                bvh_m.set_bounds(k, r);
            })
        });
        g.bench_function("move_far/grid", |b| {
            b.iter(|| {
                k = (k + 7919) % N as u32;
                let r = rects[k as usize];
                grid_m.set_bounds(k, nudge(r, PAGE_W / 2));
                grid_m.set_bounds(k, r);
            })
        });
        // Insert then remove one object: the grid lists and unlists it; a
        // static BVH has no cheaper option than a rebuild.
        g.bench_function("insert_remove/grid", |b| {
            b.iter(|| {
                grid_m.insert(u32::MAX, Rect::raw(1_000, 1_000, 9_000, 9_000), u64::MAX);
                grid_m.remove(u32::MAX);
            })
        });
        g.bench_function("insert_remove/bvh_rebuild", |b| {
            b.iter_batched(
                || (rects.clone(), z.clone()),
                |(r, z)| black_box(Bvh::build(&r, &z)),
                BatchSize::LargeInput,
            )
        });
        g.finish();
    }
}

/// The whole pick on real geometry: index candidates, then the precise test
/// top-down, as the selector will run it.
fn bench_precise_pick(c: &mut Criterion) {
    let pts = query_points(1024);
    let stroke = StrokeStyle::default();
    let tol = HitTolerance::from_device(4.0, 750.0);
    let mut g = c.benchmark_group("pick_precise");
    g.sample_size(20);
    for kind in [Scene::Uniform, Scene::Mixed, Scene::Stacked] {
        let rects = scene(kind, N);
        let paths: Vec<Path> = rects
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let mut b = Path::builder();
                if i % 2 == 0 {
                    b.ellipse(r.centre(), r.width().scale(0.5), r.height().scale(0.5));
                } else {
                    b.rect(r);
                }
                b.build()
            })
            .collect();
        // Bounds as the app caches them: geometry plus the stroke's reach.
        let grid = index_of(
            &rects
                .iter()
                .map(|r| r.inflated(Mp::new(500)))
                .collect::<Vec<_>>(),
        );
        let pick = |p: Point| {
            grid.topmost(p, Mp::new(tol.radius as i32), |k| {
                HitShape {
                    path: &paths[k as usize],
                    transform: Matrix::IDENTITY,
                    fill: Some(FillRule::NonZero),
                    stroke: Some(&stroke),
                }
                .hit(p, tol)
                .is_some()
            })
        };
        let mut i = 0usize;
        g.bench_function(format!("{kind:?}/filled"), |b| {
            b.iter(|| {
                i = (i + 1) & 1023;
                black_box(pick(pts[i]))
            })
        });
        if kind == Scene::Stacked {
            // Worst case for any index: every object's bounds contain the
            // point and none of their outlines does (unfilled outlines round
            // an empty centre), so every candidate is tested precisely.
            let centre = Point::raw(PAGE_W / 2, PAGE_H / 2);
            let hollow = |p: Point| {
                grid.topmost(p, Mp::new(tol.radius as i32), |k| {
                    HitShape {
                        path: &paths[k as usize],
                        transform: Matrix::IDENTITY,
                        fill: None,
                        stroke: Some(&stroke),
                    }
                    .hit(p, tol)
                    .is_some()
                })
            };
            g.sample_size(10);
            g.bench_function("Stacked/hollow_worst_case", |b| {
                b.iter(|| black_box(hollow(centre)))
            });
        }
    }
    g.finish();
}

criterion_group!(benches, bench_structures, bench_precise_pick);
criterion_main!(benches);
