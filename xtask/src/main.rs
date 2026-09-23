//! Development tasks for Xarast.
//!
//! Run with `cargo xtask <task>`. These never ship: `xtask` exists so that the
//! repository can regenerate its own committed assets instead of asking
//! contributors to install a pile of unrelated tools.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod export_check;

/// Icon sizes required by the freedesktop icon theme specification, plus the
/// 512 that AppImage thumbnailers prefer.
const ICON_SIZES: &[u32] = &[16, 22, 24, 32, 48, 64, 128, 256, 512];

const USAGE: &str = "\
Development tasks for Xarast

USAGE:
    cargo xtask <TASK>

TASKS:
    icons       Rasterise assets/icons/xarast.svg into the PNG sizes the
                freedesktop icon theme needs, under assets/icons/hicolor
    svg-render  <SVG> <OUT.png> [WIDTH]: parse an SVG with usvg and render
                it with resvg, resolving relative hrefs next to the file.
                The always-available browser-grade check of the .xarast
                SVG profile (research/06 §5.6)
    svg-check   [--interchange] <SVG|DIR>...: parse every SVG (a directory's
                *.svg files) with usvg and render it with resvg at 256 px
                wide; with --interchange, also fail on any `xarast`
                vocabulary. Exported SVG's validation (phase 11 T11.3.7)
    export-check [--require-tools] [--limits FILE] [--max-mean N] <DIR>:
                for every <stem>.png in DIR (our raster export over the
                paper), render <stem>.svg with resvg and <stem>.pdf with
                Poppler's pdftoppm at the PNG's size and compare; run
                `qpdf --check` on every PDF. Fails when a file does not
                parse, carries `xarast:`, qpdf complains, or the mean
                |difference| exceeds its limit (default 4/255; per-file
                limits in FILE). --require-tools fails when pdftoppm or qpdf
                is missing instead of skipping (phase 11 T11.6.2, T11.6.3)
    help        Print this help
";

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    let result = match task.as_str() {
        "icons" => icons(),
        "svg-render" => svg_render(&std::env::args().skip(2).collect::<Vec<_>>()),
        "svg-check" => svg_check(&std::env::args().skip(2).collect::<Vec<_>>()),
        "export-check" => export_check::run(&std::env::args().skip(2).collect::<Vec<_>>()),
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("unknown task: {other}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<root>/xtask`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a parent")
        .to_path_buf()
}

fn icons() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let source = root.join("assets/icons/xarast.svg");
    let svg = std::fs::read(&source)?;

    let options = resvg::usvg::Options {
        resources_dir: source.parent().map(Path::to_path_buf),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
    let natural = tree.size();

    for &size in ICON_SIZES {
        let dir = root.join(format!("assets/icons/hicolor/{size}x{size}/apps"));
        std::fs::create_dir_all(&dir)?;

        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
            .ok_or_else(|| format!("cannot allocate a {size}x{size} pixmap"))?;
        let scale = size as f32 / natural.width().max(natural.height());
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );

        let out = dir.join("xarast.png");
        pixmap.save_png(&out)?;
        println!(
            "wrote {}",
            out.strip_prefix(&root).unwrap_or(&out).display()
        );
    }

    // The scalable copy is the master itself; the theme expects it here.
    let scalable = root.join("assets/icons/hicolor/scalable/apps");
    std::fs::create_dir_all(&scalable)?;
    std::fs::copy(&source, scalable.join("xarast.svg"))?;
    println!("wrote assets/icons/hicolor/scalable/apps/xarast.svg");

    Ok(())
}

/// Renders one SVG to PNG with resvg: the conformance renderer that needs
/// no external binary. Prints the parse warnings usvg reports.
fn svg_render(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (Some(input), Some(output)) = (args.first(), args.get(1)) else {
        return Err("usage: cargo xtask svg-render <SVG> <OUT.png> [WIDTH]".into());
    };
    let source = Path::new(input);
    let svg = std::fs::read(source)?;
    let mut options = resvg::usvg::Options {
        resources_dir: source.parent().map(Path::to_path_buf),
        ..Default::default()
    };
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
    let size = tree.size();
    let width: u32 = match args.get(2) {
        Some(w) => w.parse()?,
        None => size.width().ceil() as u32,
    };
    let scale = width as f32 / size.width();
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width.max(1), height).ok_or("cannot allocate the pixmap")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(output)?;
    println!(
        "{input}: {}x{} -> {output}",
        pixmap.width(),
        pixmap.height()
    );
    Ok(())
}

fn svg_check(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let interchange = args.iter().any(|a| a == "--interchange");
    let mut files = Vec::new();
    for a in args.iter().filter(|a| *a != "--interchange") {
        let p = PathBuf::from(a);
        if p.is_dir() {
            let mut v: Vec<PathBuf> = std::fs::read_dir(&p)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")))
                .collect();
            v.sort();
            files.extend(v);
        } else {
            files.push(p);
        }
    }
    if files.is_empty() {
        return Err("usage: cargo xtask svg-check [--interchange] <SVG|DIR>...".into());
    }
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let mut failed = 0usize;
    for f in &files {
        let result = (|| -> Result<(u32, u32), Box<dyn std::error::Error>> {
            let svg = std::fs::read(f)?;
            if interchange
                && svg
                    .windows(b"xarast:".len())
                    .any(|w| w.eq_ignore_ascii_case(b"xarast:"))
            {
                return Err("carries `xarast:` vocabulary".into());
            }
            let opts = resvg::usvg::Options {
                resources_dir: f.parent().map(Path::to_path_buf),
                fontdb: options.fontdb.clone(),
                ..Default::default()
            };
            let tree = resvg::usvg::Tree::from_data(&svg, &opts)?;
            let size = tree.size();
            let scale = 256.0 / size.width();
            let height = (size.height() * scale).ceil().max(1.0) as u32;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(256, height.min(16_384))
                .ok_or("cannot allocate the pixmap")?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            Ok((pixmap.width(), pixmap.height()))
        })();
        match result {
            Ok(_) => {}
            Err(e) => {
                failed += 1;
                eprintln!("FAILED: {}: {e}", f.display());
            }
        }
    }
    println!(
        "{} SVG files: {} parsed and rendered, {failed} failed",
        files.len(),
        files.len() - failed
    );
    if failed > 0 {
        return Err(format!("{failed} SVG files failed").into());
    }
    Ok(())
}
