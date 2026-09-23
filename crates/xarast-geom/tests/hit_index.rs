//! `HitIndex` against a brute-force list, through arbitrary edit sequences.

use proptest::prelude::*;
use std::collections::HashMap;
use xarast_geom::{HitIndex, Mp, Point, Rect, RectMode};

#[derive(Clone, Debug)]
enum Op {
    Insert(u16, Rect, u64),
    Move(u16, Rect),
    SetZ(u16, u64),
    Remove(u16),
    Rebuild,
    Pick(Point, i32),
    Marquee(Rect, bool),
}

/// Rectangles of every scale the index distinguishes: dots, ordinary
/// objects, objects spanning many cells, and the odd empty one.
fn any_rect() -> impl Strategy<Value = Rect> {
    let c = -2_000_000i32..2_000_000;
    prop_oneof![
        4 => (c.clone(), c.clone(), 0i32..20_000, 0i32..20_000)
            .prop_map(|(x, y, w, h)| Rect::raw(x, y, x + w, y + h)),
        1 => (c.clone(), c.clone(), 0i32..4_000_000, 0i32..4_000_000)
            .prop_map(|(x, y, w, h)| Rect::raw(x, y, x.saturating_add(w), y.saturating_add(h))),
        1 => (c.clone(), c).prop_map(|(x, y)| Rect::raw(x, y, x, y)),
        1 => Just(Rect::EMPTY),
    ]
}

fn any_op(keys: u16) -> impl Strategy<Value = Op> {
    let key = 0u16..keys;
    let pt =
        (-2_100_000i32..2_100_000, -2_100_000i32..2_100_000).prop_map(|(x, y)| Point::raw(x, y));
    prop_oneof![
        6 => (key.clone(), any_rect(), 0u64..50).prop_map(|(k, r, z)| Op::Insert(k, r, z)),
        3 => (key.clone(), any_rect()).prop_map(|(k, r)| Op::Move(k, r)),
        1 => (key.clone(), 0u64..50).prop_map(|(k, z)| Op::SetZ(k, z)),
        2 => key.prop_map(Op::Remove),
        1 => Just(Op::Rebuild),
        4 => (pt, 0i32..50_000).prop_map(|(p, r)| Op::Pick(p, r)),
        3 => (any_rect(), any::<bool>()).prop_map(|(r, e)| Op::Marquee(r, e)),
    ]
}

fn rect_distance_sq(p: Point, r: Rect) -> f64 {
    let (px, py) = p.to_f64();
    let dx = (r.lo.x.to_f64() - px).max(px - r.hi.x.to_f64()).max(0.0);
    let dy = (r.lo.y.to_f64() - py).max(py - r.hi.y.to_f64()).max(0.0);
    dx * dx + dy * dy
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn index_matches_brute_force(ops in prop::collection::vec(any_op(64), 1..200)) {
        run(ops)?;
    }

    /// Enough objects to cross the automatic retuning thresholds and to
    /// crowd cells, so removals move other members around within lists.
    #[test]
    fn crowded_index_matches_brute_force(
        ops in prop::collection::vec(any_op(1_000), 1..2_000)
    ) {
        run(ops)?;
    }
}

fn run(ops: Vec<Op>) -> Result<(), TestCaseError> {
    {
        let mut idx: HitIndex<u16> = HitIndex::new();
        let mut model: HashMap<u16, (Rect, u64)> = HashMap::new();
        for op in ops {
            match op {
                Op::Insert(k, r, z) => {
                    prop_assert_eq!(idx.insert(k, r, z), model.insert(k, (r, z)));
                }
                Op::Move(k, r) => {
                    let had = model.get_mut(&k).map(|e| e.0 = r).is_some();
                    prop_assert_eq!(idx.set_bounds(k, r), had);
                }
                Op::SetZ(k, z) => {
                    let had = model.get_mut(&k).map(|e| e.1 = z).is_some();
                    prop_assert_eq!(idx.set_z(k, z), had);
                }
                Op::Remove(k) => {
                    prop_assert_eq!(idx.remove(k), model.remove(&k));
                }
                Op::Rebuild => idx.rebuild(),
                Op::Pick(p, r) => {
                    let got: Vec<(u16, u64)> = idx.candidates_at(p, Mp::new(r)).collect();
                    // Descending z.
                    prop_assert!(got.windows(2).all(|w| w[0].1 >= w[1].1));
                    let mut got_sorted = got.clone();
                    got_sorted.sort_unstable();
                    let r2 = f64::from(r) * f64::from(r);
                    let mut want: Vec<(u16, u64)> = model
                        .iter()
                        .filter(|(_, (b, _))| !b.is_empty() && rect_distance_sq(p, *b) <= r2)
                        .map(|(&k, &(_, z))| (k, z))
                        .collect();
                    want.sort_unstable();
                    prop_assert_eq!(got_sorted, want);
                }
                Op::Marquee(q, enclose) => {
                    let mode = if enclose {
                        RectMode::Enclose
                    } else {
                        RectMode::Touch
                    };
                    let mut got = Vec::new();
                    idx.query_rect(q, mode, &mut got);
                    got.sort_unstable();
                    let mut want: Vec<u16> = model
                        .iter()
                        .filter(|(_, (b, _))| {
                            !q.is_empty()
                                && !b.is_empty()
                                && if enclose {
                                    q.contains_rect(*b)
                                } else {
                                    q.intersects(*b)
                                }
                        })
                        .map(|(&k, _)| k)
                        .collect();
                    want.sort_unstable();
                    prop_assert_eq!(got, want);
                }
            }
            prop_assert_eq!(idx.len(), model.len());
        }
        // Everything the model holds is reported back exactly.
        for (k, v) in &model {
            prop_assert_eq!(idx.get(*k), Some(*v));
        }
    }
    Ok(())
}

#[test]
fn bulk_build_equals_incremental() {
    let entries: Vec<(u32, Rect, u64)> = (0..5_000u32)
        .map(|i| {
            let x = ((i * 7919) % 1_000) as i32 * 1_000;
            let y = ((i * 104_729) % 1_000) as i32 * 1_000;
            (i, Rect::raw(x, y, x + 3_000, y + 2_000), u64::from(i))
        })
        .collect();
    let bulk = HitIndex::from_entries(entries.iter().copied());
    let mut inc = HitIndex::new();
    for &(k, r, z) in &entries {
        inc.insert(k, r, z);
    }
    for p in [Point::raw(500_000, 500_000), Point::raw(1_234, 999_000)] {
        let a: Vec<_> = bulk.candidates_at(p, Mp::new(2_000)).collect();
        let b: Vec<_> = inc.candidates_at(p, Mp::new(2_000)).collect();
        assert_eq!(a, b);
    }
    let q = Rect::raw(100_000, 100_000, 400_000, 300_000);
    for mode in [RectMode::Touch, RectMode::Enclose] {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        bulk.query_rect(q, mode, &mut a);
        inc.query_rect(q, mode, &mut b);
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
    }
}
