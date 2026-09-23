//! `xarast-cli smoke-open`: does each document import and walk, and what
//! could the walk not draw?
//!
//! The fastest answer to "does this file open" that does not involve a
//! window, and the per-file view of the numbers the corpus tests assert
//! in aggregate.

use std::path::{Path, PathBuf};
use std::time::Instant;

use xarast_app::{DocumentId, Session, WalkStats};

use crate::Exit;
use crate::args::Args;
use crate::inputs::{expand, ms};
use crate::render::pending_summary;

/// Usage for `smoke-open`.
pub const USAGE: &str = "\
xarast-cli smoke-open — import documents and walk their scenes

USAGE:
    xarast-cli smoke-open <IN.xar|DIR>... [--quiet]

A directory stands for every .xar file below it. Each file is imported,
its scene is walked once, and a line reports the node count, the
importer's diagnostics, the primitives the walk produced and what it
could not draw yet: text, quick shapes with no cached path, undecoded
images, live effects, and unsupported clips.

OPTIONS
    --quiet   print only failures and the summary

EXIT CODE
    0 when every file imported and walked, 2 when any did not.
";

/// Parsed `smoke-open` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmokeArgs {
    /// Files or directories.
    pub inputs: Vec<PathBuf>,
    /// Print only failures and the summary.
    pub quiet: bool,
}

/// Parses `smoke-open`'s arguments.
///
/// # Errors
///
/// A message for the user when the command line is wrong.
pub fn parse(argv: &[String]) -> Result<SmokeArgs, String> {
    let mut a = SmokeArgs {
        inputs: Vec::new(),
        quiet: false,
    };
    let mut it = Args::new(argv);
    while let Some(arg) = it.next_arg() {
        match arg.as_str() {
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
    Ok(a)
}

/// What opening one document found.
#[derive(Debug, Clone, Copy)]
pub struct Smoke {
    /// Nodes in the imported tree.
    pub nodes: usize,
    /// Importer diagnostics.
    pub diagnostics: usize,
    /// Primitives the walk produced.
    pub primitives: usize,
    /// What the walk found.
    pub walk: WalkStats,
    /// Milliseconds reading and importing.
    pub open_ms: f64,
    /// Milliseconds walking.
    pub walk_ms: f64,
}

/// Opens and walks one document.
///
/// # Errors
///
/// A message when it cannot be read, imported or walked.
pub fn smoke_one(path: &Path) -> Result<Smoke, String> {
    let t0 = Instant::now();
    let mut s = Session::open(DocumentId(1), path).map_err(|e| e.to_string())?;
    let open_ms = ms(t0.elapsed());
    let t1 = Instant::now();
    let stats = s.rebuild_scene(None).map_err(|e| e.to_string())?;
    let walk_ms = ms(t1.elapsed());
    Ok(Smoke {
        nodes: s.doc.tree.node_count(),
        diagnostics: s.diagnostics().len(),
        primitives: stats.primitives(),
        walk: s.walk_stats(),
        open_ms,
        walk_ms,
    })
}

/// Runs `smoke-open`.
#[must_use]
pub fn run(a: &SmokeArgs) -> Exit {
    let files = match expand(&a.inputs) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("xarast-cli: {e}");
            return Exit::Import;
        }
    };
    let (mut ok, mut complete, mut drawn, mut failed) = (0usize, 0usize, 0usize, 0usize);
    let mut total = WalkStats::default();
    for path in &files {
        match smoke_one(path) {
            Ok(r) => {
                ok += 1;
                complete += usize::from(r.walk.is_complete());
                drawn += usize::from(r.primitives > 0);
                add(&mut total, &r.walk);
                if !a.quiet {
                    let pending = pending_summary(&r.walk);
                    println!(
                        "{}: ok, {} nodes, {} diagnostics, {} visited, {} primitives, {}, \
                         open {:.1} ms, walk {:.1} ms",
                        path.display(),
                        r.nodes,
                        r.diagnostics,
                        r.walk.visited,
                        r.primitives,
                        if pending.is_empty() {
                            "complete".to_owned()
                        } else {
                            format!("pending: {pending}")
                        },
                        r.open_ms,
                        r.walk_ms,
                    );
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("FAILED: {e}");
            }
        }
    }
    let pending = pending_summary(&total);
    println!(
        "{} files: {ok} opened ({complete} complete, {drawn} with primitives), \
         {failed} failed{}",
        files.len(),
        if pending.is_empty() {
            String::new()
        } else {
            format!("; pending in total: {pending}")
        }
    );
    if failed > 0 { Exit::Import } else { Exit::Ok }
}

fn add(t: &mut WalkStats, w: &WalkStats) {
    t.visited += w.visited;
    t.culled += w.culled;
    t.images_pending += w.images_pending;
    t.clips_unsupported += w.clips_unsupported;
    t.text_pending += w.text_pending;
    t.live_pending += w.live_pending;
    t.shapes_pending += w.shapes_pending;
}
