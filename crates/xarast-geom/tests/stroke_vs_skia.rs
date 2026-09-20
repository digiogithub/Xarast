//! Cross-check of `stroke_to_path` against `tiny-skia`'s own stroker.
//!
//! `tiny-skia` is a **test-only** dependency and never becomes a production
//! one. It earns its place here because it catches a class of error no
//! property test does: a join or a cap that is the wrong shape produces a
//! perfectly valid path with a plausible area, and only comparing coverage
//! against an independent implementation shows it up.
//!
//! The comparison rasterises our stroke outline as a fill and `tiny-skia`'s
//! stroke of the same input, then compares coverage per pixel.

use tiny_skia::{
    FillRule as SkFillRule, Paint, PathBuilder as SkBuilder, Pixmap, Stroke as SkStroke, Transform,
};
use xarast_geom::{Cap, Join, Mp, Path, Point, StrokeStyle, Tolerance};

const SIZE: u32 = 512;
/// Document millipoints per rendered pixel.
const SCALE: f32 = 1.0 / 40.0;

/// Converts one of our paths into a `tiny-skia` path in pixel space.
fn to_sk(p: &Path) -> Option<tiny_skia::Path> {
    let mut b = SkBuilder::new();
    let mut i = 0usize;
    let f = |pt: Point| {
        let (x, y) = pt.to_f64();
        // Y is up in the document and down on the pixmap, so the flip
        // happens here, at the consumer, exactly as the crate intends.
        (
            x as f32 * SCALE + 16.0,
            SIZE as f32 - (y as f32 * SCALE + 16.0),
        )
    };
    for v in p.verbs() {
        match v {
            xarast_geom::Verb::MoveTo => {
                let (x, y) = f(p.points()[i]);
                b.move_to(x, y);
                i += 1;
            }
            xarast_geom::Verb::LineTo => {
                let (x, y) = f(p.points()[i]);
                b.line_to(x, y);
                i += 1;
            }
            xarast_geom::Verb::CubicTo => {
                let (x1, y1) = f(p.points()[i]);
                let (x2, y2) = f(p.points()[i + 1]);
                let (x3, y3) = f(p.points()[i + 2]);
                b.cubic_to(x1, y1, x2, y2, x3, y3);
                i += 3;
            }
            xarast_geom::Verb::Close => b.close(),
        }
    }
    b.finish()
}

/// Rasterises a fill into an alpha mask.
fn raster_fill(p: &tiny_skia::Path) -> Vec<u8> {
    let mut pm = Pixmap::new(SIZE, SIZE).expect("a pixmap");
    let paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    pm.fill_path(p, &paint, SkFillRule::Winding, Transform::identity(), None);
    pm.pixels().iter().map(|px| px.alpha()).collect()
}

/// Rasterises `tiny-skia`'s own stroke into an alpha mask.
fn raster_stroke(p: &tiny_skia::Path, style: &StrokeStyle) -> Vec<u8> {
    let mut pm = Pixmap::new(SIZE, SIZE).expect("a pixmap");
    let paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    let mut sk = SkStroke {
        width: style.width.to_f64() as f32 * SCALE,
        miter_limit: style.mitre_limit as f32,
        ..SkStroke::default()
    };
    sk.line_cap = match style.cap_start {
        Cap::Butt => tiny_skia::LineCap::Butt,
        Cap::Round => tiny_skia::LineCap::Round,
        Cap::Square => tiny_skia::LineCap::Square,
    };
    sk.line_join = match style.join {
        Join::Mitre => tiny_skia::LineJoin::Miter,
        Join::Round => tiny_skia::LineJoin::Round,
        Join::Bevel => tiny_skia::LineJoin::Bevel,
    };
    pm.stroke_path(p, &paint, &sk, Transform::identity(), None);
    pm.pixels().iter().map(|px| px.alpha()).collect()
}

/// The stroke cases: an input path plus the style to stroke it with.
fn cases() -> Vec<(&'static str, Path, StrokeStyle)> {
    let mut v = Vec::new();
    let line = {
        let mut b = Path::builder();
        b.move_to(Point::raw(1_000, 5_000));
        b.line_to(Point::raw(15_000, 5_000));
        b.build()
    };
    let corner = {
        let mut b = Path::builder();
        b.move_to(Point::raw(2_000, 2_000));
        b.line_to(Point::raw(12_000, 2_000));
        b.line_to(Point::raw(12_000, 12_000));
        b.build()
    };
    let sharp = {
        let mut b = Path::builder();
        b.move_to(Point::raw(2_000, 2_000));
        b.line_to(Point::raw(14_000, 3_000));
        b.line_to(Point::raw(2_000, 4_000));
        b.build()
    };
    let curve = {
        let mut b = Path::builder();
        b.move_to(Point::raw(2_000, 2_000));
        b.cubic_to(
            Point::raw(2_000, 14_000),
            Point::raw(14_000, 14_000),
            Point::raw(14_000, 2_000),
        );
        b.build()
    };
    let closed = {
        let mut b = Path::builder();
        b.rect(xarast_geom::Rect::raw(3_000, 3_000, 13_000, 13_000));
        b.build()
    };
    for (name, path) in [
        ("line", line),
        ("corner", corner),
        ("sharp_corner", sharp),
        ("curve", curve),
        ("closed_rect", closed),
    ] {
        for (cap, join) in [
            (Cap::Butt, Join::Mitre),
            (Cap::Round, Join::Round),
            (Cap::Square, Join::Bevel),
            (Cap::Butt, Join::Round),
            (Cap::Round, Join::Bevel),
        ] {
            for width in [400i32, 1_200, 3_000, 6_000] {
                v.push((
                    name,
                    path.clone(),
                    StrokeStyle {
                        width: Mp::new(width),
                        cap_start: cap,
                        cap_end: cap,
                        join,
                        mitre_limit: 4.0,
                        dash: None,
                    },
                ));
            }
        }
    }
    v
}

#[test]
fn stroke_coverage_agrees_with_tiny_skia() {
    let mut worst_frac = 0.0f64;
    let mut worst_name = "";
    let all = cases();
    assert!(
        all.len() >= 100,
        "at least a hundred stroke cases, got {}",
        all.len()
    );

    for (name, path, style) in &all {
        let Some(sk_in) = to_sk(path) else { continue };
        let theirs = raster_stroke(&sk_in, style);

        let ours_path =
            xarast_geom::stroke_to_path(path, style, Tolerance(1.0)).expect("a real width");
        let Some(sk_ours) = to_sk(&ours_path) else {
            panic!("{name}: our stroke outline did not convert")
        };
        let ours = raster_fill(&sk_ours);

        // Count pixels whose coverage differs by more than 8/255. Both
        // rasterisers antialias, and a half-pixel of disagreement along a
        // long edge is a rendering artefact, not a geometry error, so the
        // threshold is on the pixel count rather than on any one pixel. The
        // measured worst case over these cases is 0.31 %, on the curve.
        let differing = ours
            .iter()
            .zip(&theirs)
            .filter(|(a, b)| a.abs_diff(**b) > 8)
            .count();
        let frac = differing as f64 / ours.len() as f64;
        if frac > worst_frac {
            worst_frac = frac;
            worst_name = name;
        }
        assert!(
            frac <= 0.005,
            "{name} at width {:?} cap {:?} join {:?}: {:.3}% of pixels differ",
            style.width,
            style.cap_start,
            style.join,
            frac * 100.0
        );
    }
    println!(
        "worst disagreement: {:.4}% on {worst_name}",
        worst_frac * 100.0
    );
}
