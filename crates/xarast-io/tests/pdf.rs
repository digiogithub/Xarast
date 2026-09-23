//! PDF export end to end (W11.4): structure, determinism, page geometry,
//! the fidelity ladder's reports, and — when Poppler's `pdftoppm` is on
//! the `PATH` — a render comparison against our own raster export.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path as GPath, Point, Rect};
use xarast_io::{
    Background, BlendFidelity, CancelFlag, Compromise, ExportArea, ExportError, ExportRequest,
    ExportSizing, FormatId, FormatOptions, NoProgress, PdfOptions, PngOptions, Registry,
    SceneSource,
};
use xarast_render::corpus::{Case, all_cases};
use xarast_render::{
    BlendFamily, Paint, PathRef, RenderQuality, Resolver, Scene, SceneBuilder, SceneNodeId,
    Transparency,
};

fn source(case: &Case) -> SceneSource<'_> {
    let side = i32::try_from(case.view.viewport.width()).unwrap() * 1000;
    SceneSource {
        scene: &case.scene,
        resolver: &case.resolver,
        area: Rect::raw(0, 0, side, side),
        paper: Rgba8::WHITE,
    }
}

fn pdf_options(fidelity: BlendFidelity, compress: bool) -> FormatOptions {
    FormatOptions::Pdf(PdfOptions {
        blend_fidelity: fidelity,
        compress,
        ..PdfOptions::default()
    })
}

fn request(dir: &Path, name: &str, options: FormatOptions) -> ExportRequest {
    let mut r = ExportRequest::new(options, dir.join(name));
    r.area = ExportArea::Drawing;
    r.sizing = ExportSizing::at_dpi(72.0);
    r
}

fn sha(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

/// The bytes as text with every non-ASCII byte replaced by `.`, so byte
/// offsets survive.
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b.is_ascii() { char::from(b) } else { '.' })
        .collect()
}

/// A structural check with no external tool: the header, every object the
/// cross-reference table lists is where it says, `startxref` points at
/// the table, and the trailer names the catalog.
fn check_structure(bytes: &[u8]) {
    let text = ascii(bytes);
    assert!(text.starts_with("%PDF-1.7\n"), "header");
    assert!(text.trim_end().ends_with("%%EOF"), "trailer");
    let sx = text.rfind("startxref").expect("startxref");
    let off: usize = text[sx + 9..]
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(text[off..].starts_with("xref"), "startxref points at xref");
    let mut lines = text[off..].lines().skip(1);
    let header: Vec<usize> = lines
        .next()
        .unwrap()
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(header[0], 0);
    let count = header[1];
    for (n, line) in lines.take(count).enumerate() {
        let entry: Vec<&str> = line.split_whitespace().collect();
        if entry[2] == "n" {
            let at: usize = entry[0].parse().unwrap();
            assert!(
                text[at..].starts_with(&format!("{n} 0 obj")),
                "object {n} at {at}"
            );
        }
    }
    assert!(text.contains("/Type /Catalog"));
    assert!(text[off..].contains("/Root 1 0 R"));
}

fn media_box(bytes: &[u8]) -> [f64; 4] {
    let text = ascii(bytes);
    let at = text.find("/MediaBox [").expect("MediaBox") + 11;
    let end = at + text[at..].find(']').unwrap();
    let v: Vec<f64> = text[at..end]
        .split_whitespace()
        .map(|x| x.parse().unwrap())
        .collect();
    [v[0], v[1], v[2], v[3]]
}

#[test]
fn every_synthetic_case_exports_to_a_well_formed_deterministic_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let cases = all_cases();
    for (i, case) in cases.iter().enumerate() {
        let src = source(case);
        let fid = if i % 2 == 0 {
            BlendFidelity::Exact
        } else {
            BlendFidelity::PreferNative
        };
        let a = request(dir.path(), "a.pdf", pdf_options(fid, true));
        let b = request(dir.path(), "b.pdf", pdf_options(fid, true));
        let ra = reg.export(&src, &a, &NoProgress).unwrap();
        reg.export(&src, &b, &NoProgress).unwrap();
        assert_eq!(sha(&a.destination), sha(&b.destination), "{}", case.name);
        let bytes = std::fs::read(&a.destination).unwrap();
        assert_eq!(ra.bytes_written, bytes.len() as u64);
        check_structure(&bytes);
        // The page is the area, to the thousandth of a point.
        let side = f64::from(case.view.viewport.width());
        let mb = media_box(&bytes);
        assert!(
            mb[0] == 0.0 && mb[1] == 0.0 && (mb[2] - side).abs() < 1e-3,
            "{}: {mb:?}",
            case.name
        );
        // Uncompressed, the same case also parses.
        let u = request(dir.path(), "u.pdf", pdf_options(fid, false));
        reg.export(&src, &u, &NoProgress).unwrap();
        check_structure(&std::fs::read(&u.destination).unwrap());
    }
}

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
    let mut b = GPath::builder();
    b.rect(Rect::new(
        Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
        Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
    ));
    PathRef::new(b.build())
}

/// A white page, a black bar, and one square per family over it.
fn blend_sheet(families: &[BlendFamily]) -> Scene {
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.fill(
        SceneNodeId(1),
        &rect_path(0.0, 40.0, 200.0, 60.0),
        FillRule::NonZero,
        Paint::Solid(Rgba8::BLACK),
    );
    for (i, f) in families.iter().enumerate() {
        let x = 10.0 + 60.0 * i as f64;
        b.push_transparency(Transparency::flat(*f, 64));
        b.fill(
            SceneNodeId(100 + i as u64),
            &rect_path(x, 20.0, x + 40.0, 80.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::rgb(200, 60, 40)),
        );
        b.pop_transparency();
    }
    b.finish().unwrap();
    scene
}

fn export_scene(
    scene: &Scene,
    dir: &Path,
    name: &str,
    options: FormatOptions,
) -> (xarast_io::ExportReport, PathBuf) {
    let res = Resolver::new();
    let src = SceneSource {
        scene,
        resolver: &res,
        area: Rect::raw(0, 0, 200_000, 100_000),
        paper: Rgba8::WHITE,
    };
    let req = request(dir, name, options);
    let rep = Registry::with_builtin()
        .export(&src, &req, &NoProgress)
        .unwrap();
    (rep, req.destination)
}

#[test]
fn stained_glass_bleach_and_contrast_are_rasterised_and_named() {
    // Phase 11 acceptance criterion 10.
    let dir = tempfile::tempdir().unwrap();
    let scene = blend_sheet(&[
        BlendFamily::StainedGlass,
        BlendFamily::Bleach,
        BlendFamily::Contrast,
    ]);
    let (rep, path) = export_scene(
        &scene,
        dir.path(),
        "exact.pdf",
        pdf_options(BlendFidelity::Exact, true),
    );
    let nodes: Vec<u64> = rep
        .compromises
        .iter()
        .filter_map(|c| match c {
            Compromise::Rasterised { node, dpi, .. } => {
                assert!((dpi - 300.0).abs() < 1e-9);
                Some(node.0)
            }
            _ => None,
        })
        .collect();
    assert_eq!(nodes, vec![100, 101, 102]);
    check_structure(&std::fs::read(path).unwrap());

    // Preferring native modes maps the two per-channel families and still
    // rasterises Contrast.
    let (rep, _) = export_scene(
        &scene,
        dir.path(),
        "native.pdf",
        pdf_options(BlendFidelity::PreferNative, false),
    );
    let mapped: Vec<(u64, String)> = rep
        .compromises
        .iter()
        .filter_map(|c| match c {
            Compromise::BlendModeApproximated { node, theirs, .. } => {
                Some((node.0, theirs.to_string()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        mapped,
        vec![(100, "Multiply".to_owned()), (101, "Screen".to_owned())]
    );
    assert_eq!(
        rep.compromises
            .iter()
            .filter(|c| matches!(c, Compromise::Rasterised { node, .. } if node.0 == 102))
            .count(),
        1
    );
}

#[test]
fn native_content_is_vector() {
    let dir = tempfile::tempdir().unwrap();
    let scene = blend_sheet(&[BlendFamily::Mix]);
    let (rep, path) = export_scene(
        &scene,
        dir.path(),
        "mix.pdf",
        pdf_options(BlendFidelity::Exact, false),
    );
    assert!(rep.compromises.is_empty(), "{:?}", rep.compromises);
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    assert!(!text.contains("/Subtype /Image"), "nothing rasterised");
    // The bar: black, the rectangle in points.
    assert!(text.contains("0 0 0 rg"), "{text}");
    assert!(text.contains("0 40 m"), "{text}");
    // The square's 75 % opacity, as a graphics state.
    assert!(text.contains("/ca 0.7490196"), "{text}");
}

#[test]
fn bleed_grows_the_page_and_sets_the_trim_box() {
    let dir = tempfile::tempdir().unwrap();
    let scene = blend_sheet(&[BlendFamily::Mix]);
    let res = Resolver::new();
    let src = SceneSource {
        scene: &scene,
        resolver: &res,
        area: Rect::raw(0, 0, 200_000, 100_000),
        paper: Rgba8::WHITE,
    };
    let mut req = request(
        dir.path(),
        "bleed.pdf",
        pdf_options(BlendFidelity::Exact, false),
    );
    req.bleed = Mp::from_pt(9.0);
    req.background = Background::Paper;
    Registry::with_builtin()
        .export(&src, &req, &NoProgress)
        .unwrap();
    let bytes = std::fs::read(&req.destination).unwrap();
    assert_eq!(media_box(&bytes), [0.0, 0.0, 218.0, 118.0]);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/TrimBox [9 9 209 109]"), "{text}");
    assert!(text.contains("/BleedBox [0 0 218 118]"), "{text}");
}

#[test]
fn a_cancelled_pdf_export_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cases = all_cases();
    let src = source(&cases[0]);
    let req = request(
        dir.path(),
        "c.pdf",
        FormatOptions::default_for(FormatId::Pdf),
    );
    let flag = CancelFlag::default();
    flag.cancel();
    let r = Registry::with_builtin().export(&src, &req, &flag);
    assert!(matches!(r, Err(ExportError::Cancelled)));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_bad_rasterising_resolution_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cases = all_cases();
    let src = source(&cases[0]);
    let req = request(
        dir.path(),
        "d.pdf",
        FormatOptions::Pdf(PdfOptions {
            rasterise_dpi: 5,
            ..PdfOptions::default()
        }),
    );
    assert!(matches!(
        Registry::with_builtin().export(&src, &req, &NoProgress),
        Err(ExportError::BadOptions { .. })
    ));
}

// ── render comparison ──────────────────────────────────────────────────────

fn pdftoppm() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("pdftoppm"))
        .find(|p| p.is_file())
}

/// Reads a binary PPM (`P6`, 8-bit).
fn read_ppm(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[s..i]).into_owned());
    }
    assert_eq!(fields[0], "P6");
    (
        fields[1].parse().unwrap(),
        fields[2].parse().unwrap(),
        bytes[i + 1..].to_vec(),
    )
}

/// Every synthetic case rendered by Poppler from our PDF against our own
/// PNG export of the same area on white. Antialiasing differs between the
/// two rasterisers, so the gate is on the mean difference and on how many
/// pixels are far off, not on exact equality.
#[test]
fn poppler_renders_what_the_raster_export_draws() {
    let Some(tool) = pdftoppm() else {
        eprintln!("pdftoppm not on PATH: render comparison skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::with_builtin();
    let mut worst = Vec::new();
    for case in all_cases() {
        let src = source(&case);
        // Rasterised objects at the comparison's own resolution, so their
        // pixels land on Poppler's grid one to one.
        let mut pdf = request(
            dir.path(),
            "c.pdf",
            FormatOptions::Pdf(PdfOptions {
                rasterise_dpi: 72,
                ..PdfOptions::default()
            }),
        );
        pdf.background = Background::Paper;
        let rep = reg.export(&src, &pdf, &NoProgress).unwrap();
        let rasterised = rep
            .compromises
            .iter()
            .any(|c| matches!(c, Compromise::Rasterised { .. }));
        if rasterised && std::env::var_os("XARAST_PDF_DUMP").is_some() {
            eprintln!("{}: {:?}", case.name, rep.compromises);
        }
        let mut png = request(
            dir.path(),
            "c.png",
            FormatOptions::Png(PngOptions::default()),
        );
        png.background = Background::Paper;
        reg.export(&src, &png, &NoProgress).unwrap();
        let out = std::process::Command::new(&tool)
            .args(["-r", "72", "-aa", "yes", "-aaVector", "yes"])
            .arg(&pdf.destination)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}: pdftoppm failed", case.name);
        // `XARAST_PDF_DUMP=<dir>` keeps every pair for inspection.
        if let Some(d) = std::env::var_os("XARAST_PDF_DUMP") {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::copy(&pdf.destination, d.join(format!("{}.pdf", case.name))).unwrap();
            std::fs::copy(&png.destination, d.join(format!("{}.png", case.name))).unwrap();
            std::fs::write(d.join(format!("{}.ppm", case.name)), &out.stdout).unwrap();
        }
        let (pw, ph, poppler) = read_ppm(&out.stdout);
        let img = image::open(&png.destination).unwrap().to_rgb8();
        assert_eq!((pw, ph), img.dimensions(), "{}", case.name);
        let ours = img.as_raw();
        let mut sum = 0u64;
        let mut far = 0usize;
        for (a, b) in poppler.chunks(3).zip(ours.chunks(3)) {
            let d = (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap();
            sum += u64::from(d);
            far += usize::from(d > 48);
        }
        let n = (pw * ph) as f64;
        let mean = sum as f64 / n;
        let far = far as f64 / n;
        worst.push((mean, far, case.name.clone(), rasterised));
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (mean, far, name, r) in worst.iter().filter(|w| !w.3).take(8) {
        eprintln!(
            "{name}: mean |d| {mean:.2}, {:.2} % far, rasterised: {r}",
            far * 100.0
        );
    }
    let mut gated = 0;
    for (mean, far, name, rasterised) in &worst {
        // Rasterised objects are ours pixel for pixel; what differs is
        // how Poppler resamples a placed image, so they are asserted in
        // the report instead (the phase test plan). A hairline is "one
        // device pixel" in both, antialiased differently by definition.
        if *rasterised || name == "aa_hairlines" {
            continue;
        }
        gated += 1;
        assert!(
            *mean < 4.0 && *far < 0.03,
            "{name}: mean |d| {mean:.2}/255, {:.2} % of pixels off by more than 48",
            far * 100.0
        );
    }
    // The corpus is feature-focused, so half of it exercises the ladder's
    // last step; the vector half must still be gated.
    assert!(gated >= 50, "{gated} of {} gated", worst.len());
}
