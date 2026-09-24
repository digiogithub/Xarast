//! `cargo xtask export-check`: exported SVG and PDF against our own PNG
//! export of the same document (phase 11 T11.6.2, T11.6.3).
//!
//! The PNG is the reference: it is the CPU rasteriser's output, which is
//! the renderer's ground truth. The SVG is rendered by resvg (a
//! browser-grade conformance renderer, MPL-2.0, which is why this lives in
//! `xtask` and not in `cargo test`) and the PDF by Poppler's `pdftoppm`,
//! both at the PNG's exact pixel size over white. Every PDF also goes
//! through `qpdf --check`.
//!
//! The measure is the mean absolute difference over the RGB channels, in
//! 1/255 units, plus the share of pixels off by more than 48 in some
//! channel (for the report only). Antialiasing, gradient sampling and text
//! rasterisation differ between renderers, so the default limit is 4/255;
//! a limits file raises it per file where a known gap is documented
//! (`docs/memory/export.md`).
//!
//! Limits file format: one `<stem> <svg|pdf> <max-mean>` per line, `#`
//! starts a comment.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

type Error = Box<dyn std::error::Error>;

/// Straight RGBA8 pixels.
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut require_tools = false;
    let mut limits_file = None::<PathBuf>;
    let mut default_limit = 4.0f64;
    let mut dir = None::<PathBuf>;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--require-tools" => require_tools = true,
            "--limits" => limits_file = Some(it.next().ok_or("--limits needs a file")?.into()),
            "--max-mean" => {
                default_limit = it.next().ok_or("--max-mean needs a number")?.parse()?
            }
            other if !other.starts_with("--") && dir.is_none() => dir = Some(other.into()),
            other => return Err(format!("export-check: unexpected `{other}`").into()),
        }
    }
    let dir = dir.ok_or(
        "usage: cargo xtask export-check [--require-tools] [--limits FILE] [--max-mean N] <DIR>",
    )?;
    let limits = match &limits_file {
        Some(f) => read_limits(f)?,
        None => BTreeMap::new(),
    };
    let pdftoppm = tool_available("pdftoppm", "-v");
    let gs = tool_available("gs", "--version");
    let qpdf = tool_available("qpdf", "--version");
    if require_tools && !(pdftoppm && gs && qpdf) {
        return Err(format!(
            "--require-tools: pdftoppm {}, gs {}, qpdf {}",
            found(pdftoppm),
            found(gs),
            found(qpdf)
        )
        .into());
    }
    if !pdftoppm && !gs {
        println!("note: neither pdftoppm nor gs found; PDF renders are not compared");
    }
    if !qpdf {
        println!("note: qpdf not found; PDF structure is not checked");
    }

    let mut stems: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    stems.sort();
    if stems.is_empty() {
        return Err(format!("{}: no reference PNGs", dir.display()).into());
    }
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    let fontdb = std::sync::Arc::new(fontdb);

    let scratch = std::env::temp_dir().join(format!("xarast-export-check-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let mut failures = Vec::new();
    let mut compared = 0usize;
    println!(
        "{:<40} {:>4} {:>10} {:>7} {:>7}  verdict",
        "file", "fmt", "size", "mean", "off>48"
    );
    for stem in &stems {
        let reference = read_png(&dir.join(format!("{stem}.png")))?;
        let svg = dir.join(format!("{stem}.svg"));
        if svg.exists() {
            let outcome = render_svg(&svg, &reference, &fontdb);
            compared += 1;
            judge(
                stem,
                "svg",
                "resvg",
                &reference,
                outcome,
                &limits,
                default_limit,
                &mut failures,
            );
        }
        let pdf = dir.join(format!("{stem}.pdf"));
        if pdf.exists() {
            if qpdf && let Err(e) = qpdf_check(&pdf) {
                failures.push(format!("{stem}.pdf: qpdf: {e}"));
                println!("{stem:<40}  pdf  qpdf --check FAILED: {e}");
            }
            // Two independent renderers; the file is judged on the one
            // that agrees best. Each has device habits the other lacks
            // (Poppler strokes anything under a pixel as a whole pixel and
            // mis-strokes one degenerate butt cap; Ghostscript antialiases
            // with 4 bits), while a fault in our file shows in both.
            let mut renders = Vec::new();
            if pdftoppm {
                renders.push(("poppler", render_pdf(&pdf, &reference, &scratch.join(stem))));
            }
            if gs {
                renders.push(("gs", render_pdf_gs(&pdf, &reference, &scratch.join(stem))));
            }
            if !renders.is_empty() {
                compared += 1;
                let mut detail = Vec::new();
                let mut best: Option<(f64, Image)> = None;
                let mut error = None;
                for (name, r) in renders {
                    match r {
                        Ok(img) => {
                            let mean = difference(&reference, &img).0;
                            detail.push(format!("{name} {mean:.2}"));
                            if best.as_ref().is_none_or(|(m, _)| mean < *m) {
                                best = Some((mean, img));
                            }
                        }
                        Err(e) => {
                            detail.push(format!("{name} failed"));
                            error = Some(e);
                        }
                    }
                }
                // A renderer that fails to read the file is a failure even
                // when the other one copes.
                let outcome = match (error, best) {
                    (Some(e), _) => Err(e),
                    (None, Some((_, img))) => Ok(img),
                    (None, None) => Err("no render".into()),
                };
                judge(
                    stem,
                    "pdf",
                    &detail.join(", "),
                    &reference,
                    outcome,
                    &limits,
                    default_limit,
                    &mut failures,
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    println!(
        "{} references, {compared} comparisons, {} failures",
        stems.len(),
        failures.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        for f in &failures {
            eprintln!("FAILED: {f}");
        }
        Err(format!("{} export checks failed", failures.len()).into())
    }
}

fn found(b: bool) -> &'static str {
    if b { "found" } else { "missing" }
}

fn tool_available(name: &str, arg: &str) -> bool {
    Command::new(name).arg(arg).output().is_ok()
}

fn read_limits(path: &Path) -> Result<BTreeMap<(String, String), f64>, Error> {
    let mut out = BTreeMap::new();
    for (n, line) in std::fs::read_to_string(path)?.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // The stem may contain spaces: the last two fields are fixed.
        let mut fields = line.rsplitn(3, char::is_whitespace);
        let (Some(limit), Some(fmt), Some(stem)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(format!(
                "{}:{}: `<stem> <svg|pdf> <max-mean>`",
                path.display(),
                n + 1
            )
            .into());
        };
        if fmt != "svg" && fmt != "pdf" {
            return Err(format!("{}:{}: format must be svg or pdf", path.display(), n + 1).into());
        }
        out.insert((stem.trim().to_owned(), fmt.to_owned()), limit.parse()?);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn judge(
    stem: &str,
    fmt: &str,
    renderers: &str,
    reference: &Image,
    outcome: Result<Image, Error>,
    limits: &BTreeMap<(String, String), f64>,
    default_limit: f64,
    failures: &mut Vec<String>,
) {
    let limit = limits
        .get(&(stem.to_owned(), fmt.to_owned()))
        .copied()
        .unwrap_or(default_limit);
    let size = format!("{}x{}", reference.width, reference.height);
    match outcome {
        Err(e) => {
            println!("{stem:<40} {fmt:>4} {size:>10}       -       -  FAILED: {e}");
            failures.push(format!("{stem}.{fmt}: {e}"));
        }
        Ok(img) => {
            let (mean, off) = difference(reference, &img);
            let ok = mean <= limit;
            println!(
                "{stem:<40} {fmt:>4} {size:>10} {mean:>7.2} {:>6.2}%  {} (limit {limit}; {renderers})",
                off * 100.0,
                if ok { "ok" } else { "OVER" },
            );
            if !ok {
                failures.push(format!(
                    "{stem}.{fmt}: mean |difference| {mean:.2}/255 over the limit {limit}"
                ));
            }
        }
    }
}

/// Mean |Δ| over RGB in 1/255 units, and the share of pixels with a
/// channel off by more than 48. Both images are composited over white.
/// A size mismatch compares the common area (resvg and Poppler may round
/// a fractional height the other way).
fn difference(a: &Image, b: &Image) -> (f64, f64) {
    let (w, h) = (a.width.min(b.width), a.height.min(b.height));
    let over_white = |p: &[u8]| -> [f64; 3] {
        let al = f64::from(p[3]) / 255.0;
        [0, 1, 2].map(|c| f64::from(p[c]) * al + 255.0 * (1.0 - al))
    };
    let (mut sum, mut off) = (0.0f64, 0usize);
    for y in 0..h {
        for x in 0..w {
            let ia = ((y * a.width + x) * 4) as usize;
            let ib = ((y * b.width + x) * 4) as usize;
            let (pa, pb) = (
                over_white(&a.rgba[ia..ia + 4]),
                over_white(&b.rgba[ib..ib + 4]),
            );
            let d = [0, 1, 2].map(|c| (pa[c] - pb[c]).abs());
            sum += d.iter().sum::<f64>();
            if d.iter().any(|v| *v > 48.0) {
                off += 1;
            }
        }
    }
    let n = f64::from(w) * f64::from(h);
    if n == 0.0 {
        return (255.0, 1.0);
    }
    (sum / (3.0 * n), off as f64 / n)
}

fn read_png(path: &Path) -> Result<Image, Error> {
    let bytes = std::fs::read(path)?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info()?;
    let mut buf = vec![0; r.output_buffer_size().ok_or("PNG too large")?];
    let info = r.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG after EXPAND".into()),
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

fn render_svg(
    path: &Path,
    reference: &Image,
    fontdb: &std::sync::Arc<resvg::usvg::fontdb::Database>,
) -> Result<Image, Error> {
    let svg = std::fs::read(path)?;
    if svg
        .windows(b"xarast:".len())
        .any(|w| w.eq_ignore_ascii_case(b"xarast:"))
    {
        return Err("carries `xarast:` vocabulary".into());
    }
    // The fonts the SVG embeds shadow installed ones, as in a browser.
    let fontdb = crate::fonts::with_embedded(fontdb, &String::from_utf8_lossy(&svg), path.parent());
    let opts = resvg::usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        fontdb,
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(&svg, &opts)?;
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(reference.width, reference.height)
        .ok_or("cannot allocate the pixmap")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    #[allow(clippy::cast_precision_loss)]
    let (sx, sy) = (
        reference.width as f32 / size.width(),
        reference.height as f32 / size.height(),
    );
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    // Opaque over white, so premultiplied and straight are the same bytes.
    Ok(Image {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: pixmap.data().to_vec(),
    })
}

fn render_pdf(path: &Path, reference: &Image, out_stem: &Path) -> Result<Image, Error> {
    let status = Command::new("pdftoppm")
        .args(["-png", "-singlefile", "-scale-to-x"])
        .arg(reference.width.to_string())
        .arg("-scale-to-y")
        .arg(reference.height.to_string())
        .arg(path)
        .arg(out_stem)
        .output()?;
    if !status.status.success() || !status.stderr.is_empty() {
        return Err(format!(
            "pdftoppm: {} {}",
            status.status,
            String::from_utf8_lossy(&status.stderr).trim()
        )
        .into());
    }
    let png = out_stem.with_extension("png");
    let img = read_png(&png);
    let _ = std::fs::remove_file(&png);
    img
}

/// Ghostscript at the PNG's exact size with 4-bit antialiasing, its best.
fn render_pdf_gs(path: &Path, reference: &Image, out_stem: &Path) -> Result<Image, Error> {
    let png = out_stem.with_extension("gs.png");
    let out = Command::new("gs")
        .args([
            "-q",
            "-dNOPAUSE",
            "-dBATCH",
            "-dSAFER",
            "-sDEVICE=png16m",
            "-dGraphicsAlphaBits=4",
            "-dTextAlphaBits=4",
            "-dPDFFitPage",
            "-dFIXEDMEDIA",
        ])
        .arg(format!("-g{}x{}", reference.width, reference.height))
        .arg(format!("-sOutputFile={}", png.display()))
        .arg(path)
        .output()?;
    if !out.status.success() || !out.stderr.is_empty() || !out.stdout.is_empty() {
        return Err(format!(
            "gs: {} {} {}",
            out.status,
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    let img = read_png(&png);
    let _ = std::fs::remove_file(&png);
    img
}

/// `qpdf --check`: exit 0 is clean, 3 is "warnings", 2 errors. Anything
/// but a clean pass fails the check.
fn qpdf_check(path: &Path) -> Result<(), Error> {
    let out = Command::new("qpdf").arg("--check").arg(path).output()?;
    if out.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&out.stdout);
        let first = text
            .lines()
            .find(|l| l.contains("WARNING") || l.contains("error"))
            .unwrap_or("");
        Err(format!("{} {first}", out.status).into())
    }
}
