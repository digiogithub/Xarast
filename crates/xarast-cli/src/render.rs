//! `xarast-cli render`: documents to PNG on the deterministic CPU backend.
//!
//! The CLI decides only *what to frame and at what size*; the pixels come
//! from `xarast_app::headless::render`, the same path the corpus tests
//! use. Framing is done by setting the session's own viewport and turning
//! the headless fit off, so a render at 100 % is exactly what the app
//! shows at 100 %.

use std::path::{Path, PathBuf};
use std::time::Instant;

use xarast_app::viewport::{nodes_rect, page_rect};
use xarast_app::{DeviceSize, DocRect, DocumentId, HeadlessOptions, Session, WalkStats, headless};
use xarast_doc::{Document, NodeKind};
use xarast_geom::Mp;
use xarast_render::RenderQuality;

use crate::Exit;
use crate::args::{Args, parse_zoom};
use crate::inputs::{expand, ms};

/// Usage for `render`.
pub const USAGE: &str = "\
xarast-cli render — render documents to PNG on the CPU backend

USAGE:
    xarast-cli render <IN.xar> -o <OUT.png> [OPTIONS]
    xarast-cli render <IN.xar|DIR>... --out-dir <DIR> [OPTIONS]

OPTIONS
    -o, --output FILE    the PNG to write (one input only)
    --out-dir DIR        write <stem>.png per input into DIR; a directory
                         input stands for every .xar file below it
    --zoom PCT           zoom as a percentage, `100` or `100%`
    --dpi N              device pixels per inch at 100 % (default 96)
    --width PX           output width in pixels
    --height PX          output height in pixels
    --frame WHAT         `drawing` (default; the page when nothing is drawn)
                         or `page`
    --quality Q          `final` (default) or `draft`
    --quiet              print only failures and the summary

SIZING
    no size, no zoom     100 %, the output is exactly the frame
    --zoom only          the output is exactly the frame at that zoom
    --width/--height     the frame is fitted into the output with a margin;
                         a missing dimension follows the frame's aspect
    size and --zoom      a fixed zoom, centred on the frame

Each line reports the output size, the display-list length, the share of
pixels that differ from the white background (`ink`), and the time spent
opening, rendering and encoding. A walk that could not draw everything
(text, quick shapes, bitmaps, live effects) says so.
";

/// The largest side a render may have.
pub const MAX_SIDE: u32 = 32_768;
/// The largest pixel count a render may have: 1 GiB of RGBA.
pub const MAX_PIXELS: u64 = 1 << 28;

/// What to frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Everything drawn; the first page when nothing is.
    Drawing,
    /// The first page.
    Page,
}

/// Parsed `render` arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderArgs {
    /// Files or directories to render.
    pub inputs: Vec<PathBuf>,
    /// `-o`: the single output.
    pub output: Option<PathBuf>,
    /// `--out-dir`: one PNG per input.
    pub out_dir: Option<PathBuf>,
    /// Zoom factor, `1.0` = 100 %.
    pub zoom: Option<f64>,
    /// Device pixels per inch at 100 %.
    pub dpi: f64,
    /// Requested width.
    pub width: Option<u32>,
    /// Requested height.
    pub height: Option<u32>,
    /// What to frame.
    pub frame: Frame,
    /// How hard to work.
    pub quality: RenderQuality,
    /// Print only failures and the summary.
    pub quiet: bool,
}

/// Parses `render`'s arguments.
///
/// # Errors
///
/// A message for the user when the command line is wrong.
pub fn parse(argv: &[String]) -> Result<RenderArgs, String> {
    let mut a = RenderArgs {
        inputs: Vec::new(),
        output: None,
        out_dir: None,
        zoom: None,
        dpi: 96.0,
        width: None,
        height: None,
        frame: Frame::Drawing,
        quality: RenderQuality::Final,
        quiet: false,
    };
    let mut it = Args::new(argv);
    while let Some(arg) = it.next_arg() {
        match arg.as_str() {
            "-o" | "--output" => a.output = Some(PathBuf::from(it.value(&arg)?)),
            "--out-dir" => a.out_dir = Some(PathBuf::from(it.value(&arg)?)),
            "--zoom" => a.zoom = Some(parse_zoom(&it.value(&arg)?)?),
            "--dpi" => {
                let d: f64 = it.parsed(&arg)?;
                if !(d.is_finite() && d > 0.0) {
                    return Err("--dpi must be a positive number".into());
                }
                a.dpi = d;
            }
            "--width" => a.width = Some(positive(it.parsed(&arg)?, &arg)?),
            "--height" => a.height = Some(positive(it.parsed(&arg)?, &arg)?),
            "--frame" => {
                a.frame = match it.value(&arg)?.as_str() {
                    "drawing" => Frame::Drawing,
                    "page" => Frame::Page,
                    other => return Err(format!("--frame takes drawing or page, not `{other}`")),
                }
            }
            "--quality" => {
                a.quality = match it.value(&arg)?.as_str() {
                    "final" => RenderQuality::Final,
                    "draft" => RenderQuality::Draft,
                    other => return Err(format!("--quality takes final or draft, not `{other}`")),
                }
            }
            "--quiet" | "-q" => a.quiet = true,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option {other}"));
            }
            other => a.inputs.push(PathBuf::from(other)),
        }
    }
    if a.inputs.is_empty() {
        return Err("no input given".into());
    }
    match (&a.output, &a.out_dir) {
        (None, None) => return Err("give -o FILE or --out-dir DIR".into()),
        (Some(_), Some(_)) => return Err("-o and --out-dir are exclusive".into()),
        (Some(_), None) if a.inputs.len() > 1 || a.inputs[0].is_dir() => {
            return Err("-o takes one input file; use --out-dir for several".into());
        }
        _ => {}
    }
    Ok(a)
}

fn positive(v: u32, flag: &str) -> Result<u32, String> {
    if v == 0 || v > MAX_SIDE {
        Err(format!("{flag} must be between 1 and {MAX_SIDE}"))
    } else {
        Ok(v)
    }
}

/// How one document is to be rendered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// The output size.
    pub size: DeviceSize,
    /// A fixed zoom centred on the frame, or `None` to fit the frame.
    pub zoom: Option<f64>,
}

/// Decides the output size and zoom for a frame rectangle, following the
/// `SIZING` rules in [`USAGE`].
///
/// # Errors
///
/// When a size has to come from an empty frame, or comes out too large.
pub fn plan(
    frame: DocRect,
    zoom: Option<f64>,
    dpi: f64,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<Plan, String> {
    let (fw, fh) = if frame.is_empty() {
        (0.0, 0.0)
    } else {
        (frame.width().to_f64(), frame.height().to_f64())
    };
    let empty = !(fw > 0.0 && fh > 0.0);
    let px = |mp: f64, z: f64| (mp * z * dpi / f64::from(Mp::PER_INCH)).ceil().max(1.0);
    let need_frame = || {
        if empty {
            Err("the document has nothing to frame; give both --width and --height".to_owned())
        } else {
            Ok(())
        }
    };
    let (w, h, z) = match (width, height) {
        (Some(w), Some(h)) => (f64::from(w), f64::from(h), zoom),
        (None, None) => {
            need_frame()?;
            let z = zoom.unwrap_or(1.0);
            (px(fw, z), px(fh, z), Some(z))
        }
        (Some(w), None) => {
            need_frame()?;
            let h = match zoom {
                Some(z) => px(fh, z),
                None => (f64::from(w) * fh / fw).round().max(1.0),
            };
            (f64::from(w), h, zoom)
        }
        (None, Some(h)) => {
            need_frame()?;
            let w = match zoom {
                Some(z) => px(fw, z),
                None => (f64::from(h) * fw / fh).round().max(1.0),
            };
            (w, f64::from(h), zoom)
        }
    };
    let max = f64::from(MAX_SIDE);
    if !(w <= max && h <= max) || w * h > MAX_PIXELS as f64 {
        return Err(format!(
            "a {w}x{h} px render is too large (at most {MAX_SIDE} px a side and \
             {MAX_PIXELS} pixels); lower --zoom or --dpi, or give --width/--height"
        ));
    }
    // Both are in 1..=MAX_SIDE here, so the casts are exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let size = DeviceSize::new(w as u32, h as u32);
    Ok(Plan { size, zoom: z })
}

/// The bounding box of what the active spread's visible, non-guide
/// layers hold — the drawing, without the pages.
///
/// `xarast_app::viewport::drawing_rect` is the root's bounds, which
/// include every page node, so a small drawing on an A4 page frames the
/// whole page. This is the rectangle "the drawing" means here.
#[must_use]
pub fn ink_rect(doc: &Document) -> DocRect {
    let tree = &doc.tree;
    let content = tree
        .children(doc.active_spread())
        .filter(|&id| matches!(tree.kind(id), Some(NodeKind::Layer(l)) if l.visible && !l.guide))
        .flat_map(|layer| tree.children(layer));
    nodes_rect(doc, content)
}

/// The rectangle a document is framed on.
#[must_use]
pub fn frame_rect(session: &Session, frame: Frame) -> DocRect {
    match frame {
        Frame::Page => page_rect(&session.doc),
        Frame::Drawing => {
            let d = ink_rect(&session.doc);
            // A quick shape with no cached path has point-like bounds: an
            // area of zero frames nothing, so fall back to the page.
            if d.is_empty() || d.width() <= Mp::ZERO || d.height() <= Mp::ZERO {
                page_rect(&session.doc)
            } else {
                d
            }
        }
    }
}

/// What rendering one document produced.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// Where the PNG went.
    pub output: PathBuf,
    /// Its size.
    pub size: DeviceSize,
    /// The zoom the frame was drawn at.
    pub zoom: f64,
    /// Display-list commands.
    pub commands: usize,
    /// Pixels that differ from the background.
    pub ink_pixels: u64,
    /// What the walk could not draw.
    pub walk: WalkStats,
    /// Milliseconds opening (read + import).
    pub open_ms: f64,
    /// Milliseconds walking and rendering.
    pub render_ms: f64,
    /// Milliseconds encoding and writing the PNG.
    pub png_ms: f64,
}

/// Renders one document.
///
/// # Errors
///
/// The exit code class and a message.
pub fn render_one(input: &Path, output: &Path, a: &RenderArgs) -> Result<Rendered, (Exit, String)> {
    let t0 = Instant::now();
    let mut session =
        Session::open(DocumentId(1), input).map_err(|e| (Exit::Import, e.to_string()))?;
    let open_ms = ms(t0.elapsed());

    let frame = frame_rect(&session, a.frame);
    let fail = |e: String| (Exit::Render, format!("{}: {e}", input.display()));
    let p = plan(frame, a.zoom, a.dpi, a.width, a.height).map_err(fail)?;
    session.quality = a.quality;
    let vp = &mut session.viewport;
    vp.set_dpi(a.dpi);
    vp.resize(p.size);
    match p.zoom {
        Some(z) => {
            vp.set_zoom(z);
            if !frame.is_empty() {
                vp.set_centre(frame.to_kurbo().center());
            }
        }
        None => vp.fit_rect(frame),
    }
    let zoom = vp.zoom();

    let opts = HeadlessOptions {
        size: p.size,
        quality: a.quality,
        fit_drawing: false,
        ..HeadlessOptions::default()
    };
    let t1 = Instant::now();
    let out = headless::render(&session, &opts).map_err(|e| fail(e.to_string()))?;
    let render_ms = ms(t1.elapsed());

    let t2 = Instant::now();
    xarast_render::golden::write_png(&out.surface, output)
        .map_err(|e| fail(format!("{}: {e}", output.display())))?;
    let png_ms = ms(t2.elapsed());

    let bg = opts.background;
    let bg = [bg.r, bg.g, bg.b, bg.a];
    let ink_pixels = out
        .surface
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| **px != bg)
        .count() as u64;

    // `headless::render` keeps its walker to itself, so the walk is
    // repeated here — outside the timed region — to learn what it skipped.
    let walk = match session.rebuild_scene(None) {
        Ok(_) => session.walk_stats(),
        Err(_) => WalkStats::default(),
    };

    Ok(Rendered {
        output: output.to_path_buf(),
        size: p.size,
        zoom,
        commands: out.commands,
        ink_pixels,
        walk,
        open_ms,
        render_ms,
        png_ms,
    })
}

/// The walk's shortfalls as a short list, empty when it drew everything.
#[must_use]
pub fn pending_summary(w: &WalkStats) -> String {
    let parts: Vec<String> = [
        ("text", w.text_pending),
        ("shapes", w.shapes_pending),
        ("images", w.images_pending),
        ("live", w.live_pending),
        ("clips-unsupported", w.clips_unsupported),
    ]
    .iter()
    .filter(|(_, n)| *n > 0)
    .map(|(k, n)| format!("{k} {n}"))
    .collect();
    parts.join(", ")
}

fn output_for(input: &Path, dir: &Path, used: &mut std::collections::HashSet<String>) -> PathBuf {
    let stem = input
        .file_stem()
        .map_or_else(|| "out".to_owned(), |s| s.to_string_lossy().into_owned());
    let mut name = format!("{stem}.png");
    let mut n = 2;
    while !used.insert(name.clone()) {
        name = format!("{stem}-{n}.png");
        n += 1;
    }
    dir.join(name)
}

/// Runs `render`.
#[must_use]
pub fn run(a: &RenderArgs) -> Exit {
    let files = match expand(&a.inputs) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("xarast-cli: {e}");
            return Exit::Import;
        }
    };
    let mut used = std::collections::HashSet::new();
    let mut worst = Exit::Ok;
    let (mut done, mut inked, mut failed) = (0usize, 0usize, 0usize);
    let (mut open_ms, mut render_ms, mut png_ms) = (0.0, 0.0, 0.0);
    for input in &files {
        let output = match (&a.output, &a.out_dir) {
            (Some(o), _) => o.clone(),
            (None, Some(d)) => output_for(input, d, &mut used),
            (None, None) => unreachable!("parse requires an output"),
        };
        match render_one(input, &output, a) {
            Ok(r) => {
                done += 1;
                inked += usize::from(r.ink_pixels > 0);
                open_ms += r.open_ms;
                render_ms += r.render_ms;
                png_ms += r.png_ms;
                if !a.quiet {
                    print_line(input, &r);
                }
            }
            Err((code, msg)) => {
                failed += 1;
                worst = worst.max(code);
                eprintln!("FAILED: {msg}");
            }
        }
    }
    if files.len() > 1 {
        println!(
            "{} files: {done} rendered ({inked} with ink, {} blank), {failed} failed; \
             open {open_ms:.1} ms, render {render_ms:.1} ms, png {png_ms:.1} ms",
            files.len(),
            done - inked,
        );
    }
    worst
}

fn print_line(input: &Path, r: &Rendered) {
    let total = u64::from(r.size.width) * u64::from(r.size.height);
    #[allow(clippy::cast_precision_loss)]
    let ink = 100.0 * r.ink_pixels as f64 / total.max(1) as f64;
    let pending = pending_summary(&r.walk);
    println!(
        "{} -> {}: {}x{} px at {:.1}%, {} cmds, ink {ink:.2}%, open {:.1} ms, \
         render {:.1} ms, png {:.1} ms{}",
        input.display(),
        r.output.display(),
        r.size.width,
        r.size.height,
        r.zoom * 100.0,
        r.commands,
        r.open_ms,
        r.render_ms,
        r.png_ms,
        if pending.is_empty() {
            String::new()
        } else {
            format!(" [not drawn: {pending}]")
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_geom::{Point, Rect};

    /// One inch by two inches.
    fn inch_rect() -> DocRect {
        Rect::new(
            Point::new(Mp::new(0), Mp::new(0)),
            Point::new(Mp::new(72_000), Mp::new(144_000)),
        )
    }

    fn argv(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn default_plan_is_the_frame_at_100_percent() {
        let p = plan(inch_rect(), None, 96.0, None, None).unwrap();
        assert_eq!(p.size, DeviceSize::new(96, 192));
        assert_eq!(p.zoom, Some(1.0));
    }

    #[test]
    fn zoom_and_dpi_scale_the_output() {
        let p = plan(inch_rect(), Some(2.0), 72.0, None, None).unwrap();
        assert_eq!(p.size, DeviceSize::new(144, 288));
    }

    #[test]
    fn one_dimension_follows_the_aspect_and_fits() {
        let p = plan(inch_rect(), None, 96.0, Some(50), None).unwrap();
        assert_eq!(p.size, DeviceSize::new(50, 100));
        assert_eq!(p.zoom, None);
        let p = plan(inch_rect(), None, 96.0, None, Some(50)).unwrap();
        assert_eq!(p.size, DeviceSize::new(25, 50));
    }

    #[test]
    fn an_empty_frame_needs_an_explicit_size() {
        assert!(plan(DocRect::EMPTY, None, 96.0, None, None).is_err());
        assert!(plan(DocRect::EMPTY, None, 96.0, Some(10), None).is_err());
        let p = plan(DocRect::EMPTY, None, 96.0, Some(10), Some(20)).unwrap();
        assert_eq!(p.size, DeviceSize::new(10, 20));
    }

    #[test]
    fn a_huge_render_is_refused() {
        assert!(plan(inch_rect(), Some(250.0), 600.0, None, None).is_err());
    }

    #[test]
    fn parse_checks_outputs() {
        assert!(parse(&argv(&["a.xar"])).is_err());
        assert!(parse(&argv(&["a.xar", "b.xar", "-o", "x.png"])).is_err());
        assert!(parse(&argv(&["a.xar", "-o", "x.png", "--out-dir", "d"])).is_err());
        assert!(parse(&argv(&["a.xar", "-o", "x.png", "--bogus"])).is_err());
        let a = parse(&argv(&[
            "a.xar",
            "-o",
            "x.png",
            "--zoom=50%",
            "--quality",
            "draft",
            "--frame",
            "page",
        ]))
        .unwrap();
        assert_eq!(a.zoom, Some(0.5));
        assert_eq!(a.quality, RenderQuality::Draft);
        assert_eq!(a.frame, Frame::Page);
    }
}
