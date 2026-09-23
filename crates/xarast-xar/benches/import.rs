//! `.xar` import time over the real corpus, in the release profile.
//!
//! The corpus is never in the repository: it is found through
//! `XARAST_XAR_CORPUS` (default `/home/user/xara-xtreme`) and the file list
//! comes from `tests/corpus/corpus.lock`, which holds only paths, sizes and
//! hashes. With no corpus the bench prints a notice and exits cleanly.
//!
//! This is `harness = false` because what it has to report is a table of 59
//! files, not one statistic. Protocol: the file is read into memory once
//! (the import budget is for decoding, not for the disk), one warm-up
//! import, then `IMPORT_RUNS` (default 7) timed imports; the median is
//! reported. "parse" is the physical layer and record tree (`analyse`),
//! "import" is the full import to a `Document` (`import`), which includes
//! the parse.
//!
//! ```text
//! XARAST_XAR_CORPUS=/path/to/xara-xtreme cargo bench -p xarast-xar --bench import
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use xarast_xar::{ImportOptions, ReaderLimits, analyse, import};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

fn median(mut xs: Vec<Duration>) -> f64 {
    xs.sort();
    xs[xs.len() / 2].as_secs_f64() * 1e3
}

fn time<F: FnMut()>(runs: usize, mut f: F) -> f64 {
    f();
    median(
        (0..runs)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed()
            })
            .collect(),
    )
}

fn main() {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    let runs: usize = std::env::var("IMPORT_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7)
        .max(1);
    let files: Vec<(u64, String)> = LOCK
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let _sha = it.next()?;
            let size = it.next()?.parse().ok()?;
            Some((size, it.collect::<Vec<_>>().join(" ")))
        })
        .collect();
    if !root.is_dir() {
        println!(
            "skipping: no .xar corpus at {} (set XARAST_XAR_CORPUS)",
            root.display()
        );
        return;
    }
    let opts = ImportOptions::default();
    let mut rows = Vec::new();
    for (size, rel) in &files {
        let Ok(bytes) = std::fs::read(root.join(rel)) else {
            println!("missing: {rel}");
            continue;
        };
        let nodes = import(&bytes, &opts).map_or(0, |(d, _)| d.tree.node_count());
        let parse = time(runs, || {
            let _ = black_box(analyse(black_box(&bytes), ReaderLimits::default()));
        });
        let full = time(runs, || {
            let _ = black_box(import(black_box(&bytes), &opts));
        });
        rows.push((*size, rel.clone(), nodes, parse, full));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    println!("# .xar import, release, median of {runs} warm runs, bytes in memory");
    println!();
    println!("| file | bytes | nodes | parse ms | import ms | MB/s |");
    println!("|---|---|---|---|---|---|");
    for (size, rel, nodes, parse, full) in &rows {
        println!(
            "| {rel} | {size} | {nodes} | {parse:.2} | {full:.2} | {:.0} |",
            *size as f64 / 1e6 / (full / 1e3)
        );
    }
    let bytes: u64 = rows.iter().map(|r| r.0).sum();
    let nodes: usize = rows.iter().map(|r| r.2).sum();
    let parse: f64 = rows.iter().map(|r| r.3).sum();
    let full: f64 = rows.iter().map(|r| r.4).sum();
    let mut sorted: Vec<f64> = rows.iter().map(|r| r.4).collect();
    sorted.sort_by(f64::total_cmp);
    println!(
        "| **all {} files** | {bytes} | {nodes} | {parse:.1} | {full:.1} | {:.0} |",
        rows.len(),
        bytes as f64 / 1e6 / (full / 1e3)
    );
    if !sorted.is_empty() {
        println!();
        println!(
            "per-file import: median {:.2} ms, p90 {:.2} ms, max {:.2} ms",
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 9 / 10],
            sorted[sorted.len() - 1]
        );
    }
}
