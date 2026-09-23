//! Raster export end to end, over the render crate's synthetic corpus:
//! determinism, metadata, backgrounds, cancellation, refusals.

use std::path::Path;

use sha2::{Digest, Sha256};
use xarast_color::Rgba8;
use xarast_geom::Rect;
use xarast_io::{
    Background, CancelFlag, Compromise, ExportArea, ExportError, ExportRequest, ExportSizing,
    FormatId, FormatOptions, JpegOptions, NoProgress, PngColour, PngOptions, Registry, SceneSource,
    WebPMode, WebPOptions,
};
use xarast_render::corpus::{Case, all_cases};

fn source(case: &Case) -> SceneSource<'_> {
    let side = i32::try_from(case.view.viewport.width()).unwrap() * 1000;
    SceneSource {
        scene: &case.scene,
        resolver: &case.resolver,
        area: Rect::raw(0, 0, side, side),
        paper: Rgba8::WHITE,
    }
}

fn request(dir: &Path, name: &str, options: FormatOptions) -> ExportRequest {
    let mut r = ExportRequest::new(options, dir.join(name));
    r.area = ExportArea::Drawing;
    r.sizing = ExportSizing::at_dpi(150.0);
    r
}

fn sha(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(&bytes))
}

#[test]
fn every_format_is_byte_identical_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let cases = all_cases();
    for case in cases.iter().step_by(7) {
        let src = source(case);
        for id in [FormatId::Png, FormatId::Jpeg, FormatId::WebP] {
            let a = request(
                dir.path(),
                &format!("a.{}", id.extension()),
                FormatOptions::default_for(id),
            );
            let b = request(
                dir.path(),
                &format!("b.{}", id.extension()),
                FormatOptions::default_for(id),
            );
            let ra = reg.export(&src, &a, &NoProgress).unwrap();
            let rb = reg.export(&src, &b, &NoProgress).unwrap();
            assert_eq!(
                sha(&a.destination),
                sha(&b.destination),
                "{} {id:?}",
                case.name
            );
            assert_eq!(
                ra.bytes_written,
                std::fs::metadata(&a.destination).unwrap().len()
            );
            assert_eq!(ra.pixels, rb.pixels);
        }
    }
}

#[test]
fn png_carries_its_size_resolution_and_colour_space() {
    let dir = tempfile::tempdir().unwrap();
    let cases = all_cases();
    let src = source(&cases[0]);
    let req = request(dir.path(), "x.png", FormatOptions::default());
    let rep = Registry::with_builtin()
        .export(&src, &req, &NoProgress)
        .unwrap();
    // The case's square, in points, at 150 dpi.
    let side = f64::from(cases[0].view.viewport.width());
    let want = (side * 150.0 / 72.0).round() as u32;
    assert_eq!(rep.pixels, (want, want));
    let dec = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(&req.destination).unwrap(),
    ));
    let r = dec.read_info().unwrap();
    let info = r.info();
    assert_eq!((info.width, info.height), (want, want));
    assert!(info.srgb.is_some());
    assert_eq!(info.pixel_dims.unwrap().xppu, 5906); // 150 dpi
    assert_eq!(info.color_type, png::ColorType::Rgba);
}

#[test]
fn streamed_and_whole_image_pngs_hold_the_same_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    for case in all_cases().iter().step_by(11) {
        let src = source(case);
        let streamed = request(dir.path(), "s.png", FormatOptions::default());
        let whole = request(
            dir.path(),
            "w.png",
            FormatOptions::Png(PngOptions {
                interlace: true,
                ..PngOptions::default()
            }),
        );
        reg.export(&src, &streamed, &NoProgress).unwrap();
        reg.export(&src, &whole, &NoProgress).unwrap();
        let a = image::open(&streamed.destination).unwrap().to_rgba8();
        let b = image::open(&whole.destination).unwrap().to_rgba8();
        assert_eq!(a, b, "{}", case.name);
    }
}

#[test]
fn backgrounds_are_honoured_and_flattening_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let cases = all_cases();
    // A case that leaves part of its area empty, and a pixel there, found
    // from a transparent PNG.
    let t = request(dir.path(), "t.png", FormatOptions::default());
    let (src, (x, y)) = cases
        .iter()
        .find_map(|case| {
            let src = source(case);
            let rep = reg.export(&src, &t, &NoProgress).unwrap();
            assert!(rep.compromises.is_empty(), "{:?}", rep.compromises);
            let img = image::open(&t.destination).unwrap().to_rgba8();
            img.enumerate_pixels()
                .find(|(_, _, p)| p.0[3] == 0)
                .map(|(x, y, _)| (src, (x, y)))
        })
        .expect("a case with a transparent pixel");

    // JPEG over a transparent background: flattened onto the paper, and
    // said so.
    let j = request(
        dir.path(),
        "t.jpg",
        FormatOptions::default_for(FormatId::Jpeg),
    );
    let rep = reg.export(&src, &j, &NoProgress).unwrap();
    assert!(rep.compromises.iter().any(|c| matches!(
        c,
        Compromise::AlphaFlattened { onto } if *onto == Rgba8::WHITE
    )));
    let jimg = image::open(&j.destination).unwrap().to_rgb8();
    for c in jimg.get_pixel(x, y).0 {
        assert!(c >= 250, "{:?}", jimg.get_pixel(x, y));
    }

    // PNG over an explicit colour: exact at the empty pixel.
    let mut c = request(dir.path(), "c.png", FormatOptions::default());
    let teal = Rgba8 {
        r: 0,
        g: 128,
        b: 128,
        a: 255,
    };
    c.background = Background::Colour(teal);
    reg.export(&src, &c, &NoProgress).unwrap();
    let cimg = image::open(&c.destination).unwrap().to_rgba8();
    assert_eq!(cimg.get_pixel(x, y).0, [0, 128, 128, 255]);

    // RGB PNG, transparent requested: flattened and reported.
    let rgb = request(
        dir.path(),
        "rgb.png",
        FormatOptions::Png(PngOptions {
            colour: PngColour::Rgb,
            ..PngOptions::default()
        }),
    );
    let rep = reg.export(&src, &rgb, &NoProgress).unwrap();
    assert!(matches!(
        rep.compromises[..],
        [Compromise::AlphaFlattened { .. }]
    ));
}

#[test]
fn a_cancelled_export_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let cases = all_cases();
    let src = source(&cases[0]);
    let flag = CancelFlag::default();
    flag.cancel();
    for id in [FormatId::Png, FormatId::Jpeg, FormatId::WebP] {
        let req = request(
            dir.path(),
            &format!("c.{}", id.extension()),
            FormatOptions::default_for(id),
        );
        assert!(matches!(
            reg.export(&src, &req, &flag),
            Err(ExportError::Cancelled)
        ));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn what_cannot_be_done_is_refused_before_rendering() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let cases = all_cases();
    let src = source(&cases[0]);
    let lossy = request(
        dir.path(),
        "l.webp",
        FormatOptions::WebP(WebPOptions {
            mode: WebPMode::Lossy { quality: 80 },
        }),
    );
    assert!(matches!(
        reg.export(&src, &lossy, &NoProgress),
        Err(ExportError::FeatureNotBuilt(_))
    ));
    let mut big = request(
        dir.path(),
        "b.webp",
        FormatOptions::default_for(FormatId::WebP),
    );
    big.sizing = ExportSizing::with_pixels(20_000, 0);
    assert!(matches!(
        reg.export(&src, &big, &NoProgress),
        Err(ExportError::BadOptions { .. })
    ));
    let bad_q = request(
        dir.path(),
        "q.jpg",
        FormatOptions::Jpeg(JpegOptions {
            quality: 0,
            ..JpegOptions::default()
        }),
    );
    assert!(reg.export(&src, &bad_q, &NoProgress).is_err());
    #[cfg(not(feature = "oxipng"))]
    {
        let opt = request(
            dir.path(),
            "o.png",
            FormatOptions::Png(PngOptions {
                optimise: true,
                ..PngOptions::default()
            }),
        );
        assert!(matches!(
            reg.export(&src, &opt, &NoProgress),
            Err(ExportError::FeatureNotBuilt(_))
        ));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(feature = "oxipng")]
#[test]
fn the_optimise_pass_shrinks_and_keeps_the_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let cases = all_cases();
    let src = source(&cases[0]);
    let plain = request(dir.path(), "p.png", FormatOptions::default());
    let opt = request(
        dir.path(),
        "o.png",
        FormatOptions::Png(PngOptions {
            optimise: true,
            ..PngOptions::default()
        }),
    );
    let a = reg.export(&src, &plain, &NoProgress).unwrap();
    let b = reg.export(&src, &opt, &NoProgress).unwrap();
    assert!(b.bytes_written <= a.bytes_written);
    let pa = image::open(&plain.destination).unwrap().to_rgba8();
    let pb = image::open(&opt.destination).unwrap().to_rgba8();
    assert_eq!(pa, pb);
    let again = request(
        dir.path(),
        "o2.png",
        FormatOptions::Png(PngOptions {
            optimise: true,
            ..PngOptions::default()
        }),
    );
    reg.export(&src, &again, &NoProgress).unwrap();
    assert_eq!(sha(&opt.destination), sha(&again.destination));
}
