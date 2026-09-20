//! Development tasks for Xarast.
//!
//! Run with `cargo xtask <task>`. These never ship: `xtask` exists so that the
//! repository can regenerate its own committed assets instead of asking
//! contributors to install a pile of unrelated tools.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
    help        Print this help
";

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    let result = match task.as_str() {
        "icons" => icons(),
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
