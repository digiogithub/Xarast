//! `xar-dump`: say exactly what is inside a `.xar` file.
//!
//! This is not a toy. It is how the corpus acceptance numbers are produced,
//! how a user reports "this file does not open", and how the next person to
//! touch the importer learns the format.
//!
//! # The clean-room rule, which is why the modes are split
//!
//! `--records`, `--tree` and `--model` print the file's **content** —
//! coordinates, colour values, text. Committing that for one of Xara's
//! design files would redistribute their artwork, which
//! `docs/11-licensing-and-clean-room.md §3.2` forbids. So those modes are
//! for interactive use and their output is never committed.
//!
//! `--stats`, `--tags` and `--corpus` print only **facts**: counts, tag
//! histograms, depths, diagnostics. That output is what the snapshot tests
//! compare, and a test greps the snapshots to keep it honest.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xarast_geom::Point;
use xarast_xar::{
    Decoded, DiagSink, ReaderLimits, RecordNode, RecordReader, analyse, decode, name_of,
    xar_dump_report,
};

const USAGE: &str = "\
xar-dump — inspect a legacy Xara .xar file

USAGE:
    xar-dump <FILE>...      [--records] [--tree] [--tags] [--stats]
                            [--json] [--max-depth N] [--limit N] [--quiet]
    xar-dump --corpus <DIR> [--json] [--fail-on warning|error]

MODES
    --stats       counts, blocks, diagnostics                (default)
    --tags        the tag histogram, with classes and names
    --records     every record: number, tag, size, offset
    --tree        the DOWN/UP tree, with decoded payloads
    --json        machine-readable output

OPTIONS
    --max-depth N  stop printing the tree below depth N
    --limit N      print at most N records
    --quiet        print nothing but the exit code
    --fail-on W    with --corpus: exit 1 on warnings, or on errors

EXIT CODES
    0  clean
    1  parsed with findings, and --fail-on said that counts
    2  a file failed to parse
    3  reserved for a document that failed validation

CLEAN ROOM
    --records and --tree print the FILE'S CONTENT and are for interactive
    use: do not commit their output. --stats, --tags and --corpus print
    only facts about a file — counts, histograms, depths, diagnostics —
    and that output is safe to commit. See docs/11-licensing-and-clean-room.md.
";

#[derive(Default)]
struct Args {
    files: Vec<PathBuf>,
    corpus: Option<PathBuf>,
    records: bool,
    tree: bool,
    tags: bool,
    stats: bool,
    json: bool,
    quiet: bool,
    max_depth: Option<usize>,
    limit: Option<usize>,
    fail_on: Option<String>,
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") || argv.is_empty() {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("xar-dump: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(dir) = &args.corpus {
        run_corpus(dir, &args)
    } else {
        run_files(&args)
    }
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--records" => a.records = true,
            "--tree" => a.tree = true,
            "--tags" => a.tags = true,
            "--stats" => a.stats = true,
            "--json" => a.json = true,
            "--quiet" => a.quiet = true,
            "--model" | "--validate" => {
                return Err(
                    "--model and --validate need the document model, which is not wired up yet"
                        .into(),
                );
            }
            "--corpus" => {
                a.corpus = Some(PathBuf::from(
                    it.next().ok_or("--corpus needs a directory")?,
                ));
            }
            "--max-depth" => {
                a.max_depth = Some(
                    it.next()
                        .ok_or("--max-depth needs a number")?
                        .parse()
                        .map_err(|_| "--max-depth needs a number")?,
                );
            }
            "--limit" => {
                a.limit = Some(
                    it.next()
                        .ok_or("--limit needs a number")?
                        .parse()
                        .map_err(|_| "--limit needs a number")?,
                );
            }
            "--fail-on" => {
                let v = it.next().ok_or("--fail-on needs warning or error")?;
                if v != "warning" && v != "error" {
                    return Err("--fail-on takes warning or error".into());
                }
                a.fail_on = Some(v.clone());
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => a.files.push(PathBuf::from(other)),
        }
    }
    if !(a.records || a.tree || a.tags) {
        a.stats = true;
    }
    Ok(a)
}

fn run_files(args: &Args) -> ExitCode {
    let mut worst = 0u8;
    for path in &args.files {
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("xar-dump: cannot read {}", path.display());
            worst = worst.max(2);
            continue;
        };
        let label = path.display().to_string();
        let report = xar_dump_report(&label, &bytes, ReaderLimits::default());
        if !report.ok() {
            worst = worst.max(2);
        } else if failed_on(args, report.errors(), report.warnings()) {
            worst = worst.max(1);
        }
        if args.quiet {
            continue;
        }
        if args.stats {
            if args.json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
        }
        if args.tags && !args.json {
            print!("{}", report.tag_table());
        }
        if args.records {
            print_records(&bytes, args);
        }
        if args.tree {
            print_tree(&bytes, args);
        }
    }
    ExitCode::from(worst)
}

fn failed_on(args: &Args, errors: u64, warnings: u64) -> bool {
    match args.fail_on.as_deref() {
        Some("error") => errors > 0,
        Some("warning") => errors > 0 || warnings > 0,
        _ => false,
    }
}

/// Prints the flat record sequence. **File content**: interactive use only.
fn print_records(bytes: &[u8], args: &Args) {
    let Ok(mut reader) = RecordReader::new(bytes, ReaderLimits::default()) else {
        return;
    };
    let mut printed = 0usize;
    while let Some(rec) = reader.next_record() {
        match rec {
            Ok(r) => {
                println!(
                    "#{:<7} tag {:<5} {:<40} size {:<8} at {:<9} {}",
                    r.number,
                    r.tag,
                    name_of(r.tag).unwrap_or("(undefined)"),
                    r.data.len(),
                    r.file_offset,
                    if r.compressed { "compressed" } else { "" }
                );
            }
            Err(e) => {
                println!("error: {e}");
                break;
            }
        }
        printed += 1;
        if args.limit.is_some_and(|n| printed >= n) {
            break;
        }
    }
}

/// Prints the decoded tree. **File content**: interactive use only.
fn print_tree(bytes: &[u8], args: &Args) {
    let Ok(a) = analyse(bytes, ReaderLimits::default()) else {
        return;
    };
    let mut diags = DiagSink::new();
    let mut printed = 0usize;
    a.tree.walk(&mut |node: &RecordNode, depth: usize| {
        if args.max_depth.is_some_and(|m| depth > m) {
            return;
        }
        if args.limit.is_some_and(|n| printed >= n) {
            return;
        }
        printed += 1;
        let r = &node.record;
        let decoded = decode(r.tag, &r.data, Point::ORIGIN, &mut diags, (r.number, r.tag));
        let what = match decoded {
            Ok(Decoded::Unhandled { tag, size }) => {
                format!("(no decoder, tag {tag}, {size} bytes)")
            }
            Ok(d) => format!("{d:?}"),
            Err(e) => format!("(error: {e})"),
        };
        println!(
            "{:indent$}#{} {} {}",
            "",
            r.number,
            name_of(r.tag).unwrap_or("(undefined)"),
            what,
            indent = depth * 2
        );
    });
}

fn run_corpus(dir: &Path, args: &Args) -> ExitCode {
    let mut files: Vec<PathBuf> = Vec::new();
    collect(dir, &mut files, 0);
    files.sort();
    let mut worst = 0u8;
    let mut rows = Vec::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let label = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let r = xar_dump_report(&label, &bytes, ReaderLimits::default());
        if !r.ok() {
            worst = worst.max(2);
        } else if failed_on(args, r.errors(), r.warnings()) {
            worst = worst.max(1);
        }
        rows.push(r);
    }
    if args.quiet {
        return ExitCode::from(worst);
    }
    if args.json {
        println!("{{\"files\":[");
        for (i, r) in rows.iter().enumerate() {
            println!("{}{}", if i > 0 { "," } else { "" }, r.to_json());
        }
        println!("],\"count\":{}}}", rows.len());
        return ExitCode::from(worst);
    }
    println!(
        "{:<44} {:>9} {:>5} {:>6} {:>7} {:>8} {:>6}",
        "file", "records", "tags", "depth", "blocks", "handled", "status"
    );
    for r in &rows {
        println!(
            "{:<44} {:>9} {:>5} {:>6} {:>3}/{:<3} {:>8} {:>6}",
            truncate(&r.label, 44),
            r.records,
            r.distinct_tags,
            r.max_depth,
            r.blocks_ok,
            r.blocks,
            r.handled,
            if r.ok() { "ok" } else { "FAIL" }
        );
    }
    let ok = rows.iter().filter(|r| r.ok()).count();
    let records: u64 = rows.iter().map(|r| u64::from(r.records)).sum();
    let blocks: usize = rows.iter().map(|r| r.blocks).sum();
    let blocks_ok: usize = rows.iter().map(|r| r.blocks_ok).sum();
    let mut tags = std::collections::BTreeSet::new();
    for r in &rows {
        tags.extend(r.histogram.keys().copied());
    }
    println!(
        "\n{ok} of {} files parsed, {records} records, {} distinct tags, {blocks_ok}/{blocks} blocks verified",
        rows.len(),
        tags.len()
    );
    ExitCode::from(worst)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xar")) {
            out.push(p);
        }
    }
}

fn truncate(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or(s)
}
