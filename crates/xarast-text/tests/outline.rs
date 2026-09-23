//! W9.6 (T9.6.1-T9.6.2): glyph outlines through skrifa's pen into
//! `kurbo::BezPath`, with variations, cached.

mod common;

use std::sync::Arc;

use common::*;
use kurbo::{PathEl, Shape};
use xarast_text::{FontQuery, FontVariation, ParagraphStyle, StoryMode};

fn wght(v: f32) -> [FontVariation; 1] {
    [FontVariation {
        tag: *b"wght",
        value: v,
    }]
}

/// Glyph 2 of the synthetic variable font is `I`: a rectangle whose right
/// edge moves with the weight axis (see `tests/fonts/make_variable.py`).
const GLYPH_I: u32 = 2;
const GLYPH_O: u32 = 3;

#[test]
fn a_rectangle_glyph_follows_the_weight_axis() {
    let db = pinned_db();
    let face = db
        .query(&FontQuery::new("Xarast Test Variable"))
        .unwrap()
        .face;
    let bbox = |v: &[FontVariation]| db.glyph_outline(face, GLYPH_I, v).unwrap().bounding_box();
    let d = bbox(&[]);
    assert_eq!((d.x0, d.y0, d.x1, d.y1), (100.0, 0.0, 200.0, 700.0));
    assert_eq!(bbox(&wght(900.0)).x1, 300.0);
    assert_eq!(bbox(&wght(100.0)).x1, 150.0);
    assert_eq!(bbox(&wght(650.0)).x1, 250.0, "halfway to the heavy master");
    assert_eq!(bbox(&wght(5000.0)).x1, 300.0, "clamped to the axis");
    assert_eq!(db.normalized_coords(face, &wght(900.0)), [16384]);
    assert_eq!(db.normalized_coords(face, &[]), [0]);
}

#[test]
fn truetype_quadratics_come_through_as_quads() {
    let db = pinned_db();
    let face = db
        .query(&FontQuery::new("Xarast Test Variable"))
        .unwrap()
        .face;
    let o = db.glyph_outline(face, GLYPH_O, &[]).unwrap();
    let quads = o
        .elements()
        .iter()
        .filter(|e| matches!(e, PathEl::QuadTo(..)))
        .count();
    assert_eq!(quads, 4);
    assert!(matches!(o.elements().last(), Some(PathEl::ClosePath)));
    let b = o.bounding_box();
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (50.0, 0.0, 450.0, 500.0));
}

#[test]
fn cff_outlines_come_through_as_cubics() {
    let db = pinned_db();
    let face = db.query(&FontQuery::new("Noto Sans CJK JP")).unwrap().face;
    let s = xarast_text::Shaper::new(db.clone());
    let l = lay(
        &s,
        "日",
        &[run("日", "Noto Sans CJK JP", 10)],
        ParagraphStyle::default(),
        StoryMode::Point,
    );
    let (r, g) = l.glyphs().next().unwrap();
    assert_eq!(r.face, face);
    let o = db.glyph_outline(face, g.id, &[]).unwrap();
    assert!(o.elements().len() > 4);
    assert!(
        o.elements()
            .iter()
            .all(|e| !matches!(e, PathEl::QuadTo(..)))
    );
    let b = o.bounding_box();
    assert!(
        b.x0 > 0.0 && b.x1 < 1000.0 && b.y1 > 500.0,
        "an ideograph inside its em: {b:?}"
    );
}

#[test]
fn a_space_has_an_empty_outline_and_a_missing_glyph_none() {
    let db = pinned_db();
    let face = db.query(&FontQuery::new("Noto Sans")).unwrap().face;
    let space = db.glyph_outline(face, 1, &[]).unwrap();
    assert!(space.elements().is_empty());
    assert!(db.glyph_outline(face, 60_000, &[]).is_none());
}

#[test]
fn outlines_are_cached_by_face_glyph_and_coordinates() {
    let db = pinned_db();
    let face = db
        .query(&FontQuery::new("Xarast Test Variable"))
        .unwrap()
        .face;
    let a = db.glyph_outline(face, GLYPH_I, &[]).unwrap();
    let b = db.glyph_outline(face, GLYPH_I, &[]).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "second lookup is the cached path");
    // Explicit default coordinates and none at all are the same instance.
    let c = db.glyph_outline_normalized(face, GLYPH_I, &[0]).unwrap();
    assert!(Arc::ptr_eq(&a, &c));
    let heavy = db.glyph_outline(face, GLYPH_I, &wght(900.0)).unwrap();
    assert!(!Arc::ptr_eq(&a, &heavy));
    let heavy2 = db
        .glyph_outline_normalized(face, GLYPH_I, &[16384])
        .unwrap();
    assert!(Arc::ptr_eq(&heavy, &heavy2));
}

#[test]
fn a_placed_glyph_maps_to_story_space_by_its_run_transform() {
    let s = shaper();
    let text = "II";
    let mut r = run(text, "Xarast Test Variable", 20);
    r.aspect = 1.5;
    let l = lay(&s, text, &[r], ParagraphStyle::default(), StoryMode::Point);
    let db = s.fonts();
    let (run, g) = l.glyphs().nth(1).unwrap();
    let upem = db.units_per_em(run.face);
    let path = db
        .glyph_outline_normalized(run.face, g.id, &run.coords)
        .unwrap();
    let placed = (run.glyph_transform(g, upem) * (*path).clone()).bounding_box();
    // 20 pt = 20 000 mp per 1000 units: 20 mp per unit, ×1.5 horizontally.
    // The second I starts at one advance: 300 units × 20 × 1.5 = 9 000 mp.
    assert_eq!(g.x.raw(), 9_000);
    assert_eq!(placed.x0, 9_000.0 + 100.0 * 30.0);
    assert_eq!(placed.x1, 9_000.0 + 200.0 * 30.0);
    assert_eq!(placed.y1, 700.0 * 20.0);
}
