//! The phase-9 acceptance fixture (W9.7, XARA-US-0050): the fourteen
//! `TextDesigns/` files of the corpus, through the binaries a user runs.
//!
//! - **Tags** (T9.7.1, criterion 3): `xar-dump --tags` reports no text tag
//!   without a decoder in 2100-2117, 2200-2204, 2900-2920 and 4200-4207.
//! - **Golden renders** (T9.7.2, criterion 2): `xarast-cli render` on the
//!   CPU backend, with the pinned test fonts, renders every file with no
//!   failure and nothing left undrawn, and each render is **exactly** its
//!   golden (gate A, `docs/memory/render.md`). The corpus is Xara's
//!   artwork and each file carries a bitmap of the original's own
//!   rendering, so neither the files nor our renders of them are
//!   committed: the golden is the SHA-256 of the rendered pixels
//!   (`xarast_render::golden::digest`), a fact derived from them.
//!   `XARAST_UPDATE_GOLDEN=1` rewrites `tests/golden/text_designs.sha256`;
//!   a mismatch leaves the render in the temporary directory to look at.
//! - **No missing-glyph boxes** (criterion 2): no visible character of any
//!   story is shaped to glyph 0 with the pinned fonts.
//!
//! The `.xar` → `.xarast` → reload → render comparison (T9.7.3, criterion
//! 12) runs over all 59 corpus files, these 14 included, in
//! `xarast-app/tests/xarast_roundtrip.rs`.
//!
//! The corpus is found through `XARAST_XAR_CORPUS`; without it the tests
//! skip with a notice (`XARAST_CORPUS_REQUIRED=1` makes that a failure).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");
const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/text_designs.sha256"
);
const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../xarast-text/tests/fonts");
/// Twice the size of 100 %: small text keeps enough pixels for the digest
/// to see a sub-point move.
const ZOOM: &str = "200";

/// The text tag ranges acceptance criterion 3 names.
const TEXT_RANGES: [(u32, u32); 4] = [(2100, 2117), (2200, 2204), (2900, 2920), (4200, 4207)];

/// The corpus root and the fourteen `TextDesigns/` paths of the lock.
fn text_designs() -> Option<(PathBuf, Vec<String>)> {
    let required = std::env::var("XARAST_CORPUS_REQUIRED").as_deref() == Ok("1");
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    let files: Vec<String> = LOCK
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            it.next()?;
            it.next()?;
            Some(it.collect::<Vec<_>>().join(" "))
        })
        .filter(|rel| rel.starts_with("TextDesigns/"))
        .collect();
    assert_eq!(files.len(), 14, "the lock lists fourteen TextDesigns files");
    if !root.join("TextDesigns").is_dir() {
        assert!(!required, "XARAST_CORPUS_REQUIRED=1 but no corpus");
        eprintln!("skipping: no .xar corpus (set XARAST_XAR_CORPUS)");
        return None;
    }
    Some((root, files))
}

fn run(bin: &str, args: &[&std::ffi::OsStr]) -> Output {
    Command::new(bin)
        .args(args)
        // Pinned fonts: a test never depends on the machine's.
        .env("XARAST_FONT_DIR", FONTS)
        .output()
        .expect("spawn")
}

#[test]
fn xar_dump_reports_no_unhandled_text_tag() {
    let Some((root, files)) = text_designs() else {
        return;
    };
    let mut unhandled = Vec::new();
    // tag -> (files using it, records)
    let mut inventory: BTreeMap<u32, (String, usize, u64)> = BTreeMap::new();
    let mut per_file = String::new();
    for rel in &files {
        let path = root.join(rel);
        let o = run(
            env!("CARGO_BIN_EXE_xar-dump"),
            &["--tags".as_ref(), path.as_os_str()],
        );
        assert!(o.status.success(), "{rel}: xar-dump failed: {o:?}");
        let out = String::from_utf8_lossy(&o.stdout);
        let mut rows = 0usize;
        let mut mine = Vec::new();
        for line in out.lines().skip(1) {
            // `   tag      count  class      name`, the name prefixed with
            // `!` when no decoder exists for the tag.
            let cols: Vec<&str> = line.split_whitespace().collect();
            let (Some(tag), Some(count), Some(name)) = (
                cols.first().and_then(|t| t.parse::<u32>().ok()),
                cols.get(1).and_then(|c| c.parse::<u64>().ok()),
                cols.get(3),
            ) else {
                continue;
            };
            rows += 1;
            if !TEXT_RANGES.iter().any(|(a, b)| (*a..=*b).contains(&tag)) {
                continue;
            }
            let e = inventory
                .entry(tag)
                .or_insert_with(|| (name.trim_start_matches('!').to_owned(), 0, 0));
            e.1 += 1;
            e.2 += count;
            mine.push(tag);
            if name.starts_with('!') {
                unhandled.push(format!("{rel}: {tag} {name}"));
            }
        }
        assert!(rows > 0, "{rel}: no tag table");
        mine.sort_unstable();
        let _ = writeln!(per_file, "{rel}: {mine:?}");
    }
    let mut table = String::new();
    for (tag, (name, n, records)) in &inventory {
        let _ = writeln!(
            table,
            "{tag:>5}  {name:<32} {n:>2} files  {records:>5} records"
        );
    }
    println!("text tags in the TextDesigns files:\n{table}\n{per_file}");
    assert!(unhandled.is_empty(), "unhandled text tags: {unhandled:#?}");
}

/// `file  WxH  sha256` rows of the committed golden, by file.
fn read_golden() -> BTreeMap<String, (String, String)> {
    let Ok(text) = std::fs::read_to_string(GOLDEN) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let digest = it.next()?.to_owned();
            let size = it.next()?.to_owned();
            let rel = it.collect::<Vec<_>>().join(" ");
            Some((rel, (size, digest)))
        })
        .collect()
}

#[test]
fn every_text_design_renders_exactly_its_golden() {
    let Some((root, files)) = text_designs() else {
        return;
    };
    let update = std::env::var("XARAST_UPDATE_GOLDEN").as_deref() == Ok("1");
    let dir = std::env::temp_dir().join(format!("xarast-cli-text-designs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let input = root.join("TextDesigns");
    let o = run(
        env!("CARGO_BIN_EXE_xarast-cli"),
        &[
            "render".as_ref(),
            input.as_os_str(),
            "--out-dir".as_ref(),
            dir.as_os_str(),
            "--zoom".as_ref(),
            ZOOM.as_ref(),
        ],
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "render failed:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("14 files: 14 rendered (14 with ink, 0 blank), 0 failed"),
        "{stdout}"
    );
    assert!(!stdout.contains("[not drawn:"), "{stdout}");
    assert!(!stderr.contains("FAILED"), "{stderr}");

    let golden = read_golden();
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    let mut keep = false;
    for rel in &files {
        let stem = Path::new(rel).file_stem().unwrap().to_string_lossy();
        let png = dir.join(format!("{stem}.png"));
        let s = xarast_render::golden::read_png(&png).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let size = format!("{}x{}", s.width(), s.height());
        let digest = xarast_render::golden::digest(&s);
        match golden.get(rel) {
            Some((gs, gd)) if *gs == size && *gd == digest => {}
            Some((gs, gd)) if !update => {
                keep = true;
                failures.push(format!(
                    "{rel}: {size} {digest}, golden {gs} {gd} (render kept at {})",
                    png.display()
                ));
            }
            None if !update => failures.push(format!("{rel}: no golden")),
            _ => {}
        }
        rows.push(format!("{digest}  {size}  {rel}"));
    }
    if update {
        let mut text = String::from(
            "# SHA-256 of `xarast-cli render --zoom 200` (CPU, pinned fonts in\n\
             # crates/xarast-text/tests/fonts) of each TextDesigns corpus file, as\n\
             # `xarast_render::golden::digest` computes it. Derived facts only: the\n\
             # files and their renders are never committed. Regenerate with\n\
             # XARAST_UPDATE_GOLDEN=1 (tests/text_designs.rs).\n\
             #\n# sha256  size  path\n",
        );
        for r in &rows {
            text.push_str(r);
            text.push('\n');
        }
        std::fs::write(GOLDEN, text).unwrap();
        eprintln!("golden digests written to {GOLDEN}");
    }
    if !keep {
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn no_visible_character_is_a_missing_glyph_box() {
    use xarast_app::{DocumentId, Session, fonts};
    let Some((root, files)) = text_designs() else {
        return;
    };
    let service = fonts::FontService::from_dir(Path::new(FONTS));
    let mut boxes = Vec::new();
    let mut glyphs = 0usize;
    for rel in &files {
        let session =
            Session::open(DocumentId(1), &root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let tree = &session.doc.tree;
        let stories: Vec<_> = tree
            .preorder(tree.root())
            .filter(|&n| matches!(tree.kind(n), Some(xarast_doc::NodeKind::TextStory(_))))
            .collect();
        assert!(!stories.is_empty(), "{rel}: no story");
        for story in stories {
            let map = xarast_app::text_tool::caret_map(&session.doc, story, &service)
                .unwrap_or_else(|| panic!("{rel}: story {story:?} has no layout"));
            let text = map.text();
            for line in &map.layout().lines {
                for g in line.runs.iter().flat_map(|r| &r.glyphs) {
                    glyphs += 1;
                    let c = text[g.cluster..].chars().next().unwrap_or(' ');
                    if g.id == 0 && !c.is_whitespace() && !c.is_control() {
                        boxes.push(format!("{rel}: {c:?} (U+{:04X})", u32::from(c)));
                    }
                }
            }
        }
    }
    assert!(glyphs > 0);
    assert!(boxes.is_empty(), "missing-glyph boxes: {boxes:#?}");
}
