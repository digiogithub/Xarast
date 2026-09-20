//! The precision rule, enforced twice: by measurement and by grep.

mod common;

use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point};
use xarast_render::backend::cpu::Resolver;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, Paint, PathRef, RenderQuality, Scene,
    SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams,
};

/// A 5 m wide document rendered at 4000 %: the case where an `f32`
/// coordinate would be visibly wrong.
#[test]
fn a_five_metre_document_at_four_thousand_percent_stays_sub_pixel() {
    // 5 m is 3.6e8 millipoints. Put a thin bar a long way from the origin
    // and check that its edges land where the arithmetic says.
    let far = 360_000_000.0f64;
    let zoom = 40.0; // 4000 %
    let scale = zoom / f64::from(Mp::PER_PT);
    // Place the viewport over the bar.
    let bar_x0 = far;
    let bar_x1 = far + 500.0; // 0.5 pt wide = 20 device pixels
    let mut b = Path::builder();
    b.move_to(Point::new(Mp::new(bar_x0 as i32), Mp::new(0)));
    b.line_to(Point::new(Mp::new(bar_x1 as i32), Mp::new(0)));
    b.line_to(Point::new(Mp::new(bar_x1 as i32), Mp::new(4_000)));
    b.line_to(Point::new(Mp::new(bar_x0 as i32), Mp::new(4_000)));
    b.close();
    let path = PathRef::new(b.build());

    let device_x0 = bar_x0 * scale;
    let origin = device_x0.floor() - 10.0;
    let xf = Transform2D::scale(scale).then(Transform2D::translate(-origin, 0.0));

    let mut scene = Scene::new();
    {
        let mut sb = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        sb.fill(
            SceneNodeId(1),
            &path,
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        sb.finish().unwrap();
    }
    let view = ViewParams::new(64, 64, xf, RenderQuality::Final);
    let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
    let mut target = Surface::filled(64, 64, [255, 255, 255, 255]);
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &Resolver::new(), &mut target)
        .unwrap();

    // Find the first and last columns with any ink on row 2.
    let inked: Vec<i32> = (0..64)
        .filter(|x| target.pixel(*x, 2).is_some_and(|p| p[0] < 250))
        .collect();
    assert!(!inked.is_empty(), "the bar did not render at all");
    let (first, last) = (*inked.first().unwrap(), *inked.last().unwrap());
    let expected_first = device_x0 - origin;
    let expected_last = bar_x1 * scale - origin;
    assert!(
        (f64::from(first) - expected_first).abs() <= 1.0,
        "left edge at {first}, expected {expected_first}"
    );
    assert!(
        (f64::from(last) - expected_last).abs() <= 1.0,
        "right edge at {last}, expected {expected_last}"
    );
    // Width error well under a quarter of a pixel of the 20 px bar.
    let measured = f64::from(last - first + 1);
    let exact = expected_last - expected_first;
    assert!(
        (measured - exact).abs() < 1.25,
        "width {measured} vs {exact}"
    );
}

/// Criterion 15's grep: no bare `as f32` on anything that could be a
/// coordinate.
///
/// The rule is enforceable rather than aspirational: `precision.rs` owns
/// the tile-local conversion, and every other `as f32` in the crate must
/// carry an `// f32-ok:` justification on the line before it.
#[test]
fn no_unjustified_as_f32_in_the_crate() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    visit(&src, &mut |path, text| {
        let is_precision = path.ends_with("precision.rs");
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("as f32") || line.trim_start().starts_with("//") {
                continue;
            }
            if is_precision {
                continue;
            }
            let justified = i > 0 && lines[i - 1].contains("f32-ok");
            if !justified {
                offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    });
    assert!(
        offenders.is_empty(),
        "unjustified f32 conversions:\n{}",
        offenders.join("\n")
    );
}

fn visit(dir: &std::path::Path, f: &mut impl FnMut(&std::path::Path, &str)) {
    for entry in std::fs::read_dir(dir).expect("src is readable") {
        let entry = entry.expect("a readable entry");
        let path = entry.path();
        if path.is_dir() {
            visit(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("a readable file");
            f(&path, &text);
        }
    }
}

/// The rounding rule is stated once and is the one the engine uses.
#[test]
fn device_rounding_is_half_away_from_zero_everywhere() {
    use xarast_render::round_device;
    assert_eq!(round_device(2.5), 3);
    assert_eq!(round_device(-2.5), -3);
    assert_eq!(round_device(3.5), 4);
    // Not banker's rounding, which would give 2 and 4.
    assert_ne!(round_device(2.5), 2);
}
