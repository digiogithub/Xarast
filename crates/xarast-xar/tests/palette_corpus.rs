//! Palette resolution against the corpus (phase 8, W8.1).
//!
//! Every `TAG_DEFINECOMPLEXCOLOUR` carries the 8-bit RGB the original
//! computed for that entry (`research/02 §5.10.1`). Resolving each entry
//! through `ColourTable` — tints, shades and links included — and
//! quantising the original's way must reproduce it. That makes the corpus
//! an oracle for the conversion and derived-colour formulas.
//!
//! No corpus byte enters the repository: files are read from
//! `XARAST_XAR_CORPUS`, and the test skips when it is absent (fails when
//! `XARAST_CORPUS_REQUIRED=1`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use xarast_color::ColourKind;
use xarast_xar::ImportOptions;

fn corpus_files() -> Option<Vec<PathBuf>> {
    let required = std::env::var("XARAST_CORPUS_REQUIRED").as_deref() == Ok("1");
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if !root.is_dir() {
        assert!(
            !required,
            "XARAST_CORPUS_REQUIRED=1 but the corpus is absent"
        );
        eprintln!("corpus not found at {}; skipping", root.display());
        return None;
    }
    let mut out = Vec::new();
    for dir in ["testfiles", "Designs", "Templates", "TextDesigns"] {
        collect(&root.join(dir), &mut out);
    }
    out.sort();
    Some(out)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("xar") || x.eq_ignore_ascii_case("web"))
        {
            out.push(p);
        }
    }
}

fn kind_name(k: &ColourKind) -> &'static str {
    match k {
        ColourKind::Normal => "normal",
        ColourKind::Spot => "spot",
        ColourKind::Tint { .. } => "tint",
        ColourKind::Linked => "linked",
        ColourKind::Shade { .. } => "shade",
    }
}

#[test]
fn every_palette_entry_resolves_to_its_cached_rgb() {
    let Some(files) = corpus_files() else { return };
    // (kind, model) -> (entries, exact, worst channel error)
    let mut stats: BTreeMap<(&str, String), (usize, usize, u8)> = BTreeMap::new();
    let mut mismatches = Vec::new();
    // How many entries plain round-to-nearest would get wrong: the reason
    // the original's quantiser is reproduced.
    let mut rounding_would_miss = 0usize;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let Ok((doc, _)) = xarast_xar::import(&bytes, &ImportOptions::default()) else {
            continue;
        };
        let table = &doc.resources.colours;
        assert!(
            table.validate().is_empty(),
            "{}: broken palette",
            f.display()
        );
        for (id, def) in table.iter() {
            let got = table.resolve(id).to_rgba8_packed();
            let want = def.cached_rgb;
            let rounded = table.resolve(id).to_rgba8();
            if (rounded.r, rounded.g, rounded.b) != (want.r, want.g, want.b) {
                rounding_would_miss += 1;
            }
            let err = got
                .r
                .abs_diff(want.r)
                .max(got.g.abs_diff(want.g))
                .max(got.b.abs_diff(want.b));
            let e = stats
                .entry((kind_name(&def.kind), format!("{:?}", def.model)))
                .or_insert((0, 0, 0));
            e.0 += 1;
            if err == 0 {
                e.1 += 1;
            } else {
                mismatches.push(format!(
                    "{}: {:?} {:?} {:?} {:?} got {got:?} want {want:?}",
                    f.file_name().unwrap().to_string_lossy(),
                    def.name,
                    def.kind,
                    def.model,
                    def.components
                ));
            }
            e.2 = e.2.max(err);
        }
    }
    let mut report = String::from("kind model entries exact worst\n");
    let mut total = (0, 0);
    for ((k, m), (n, exact, worst)) in &stats {
        report.push_str(&format!("{k} {m} {n} {exact} {worst}\n"));
        total.0 += n;
        total.1 += exact;
    }
    report.push_str(&format!("total {} {}\n", total.0, total.1));
    report.push_str(&format!(
        "round-to-nearest would miss {rounding_would_miss}\n"
    ));
    println!("{report}");
    for m in mismatches.iter().take(40) {
        println!("{m}");
    }
    assert!(total.0 > 0, "no palette entries in the corpus?");
    // Measured: every entry matches exactly (see docs/memory/colour.md).
    assert!(
        mismatches.is_empty(),
        "{} of {} entries differ from their cached RGB",
        mismatches.len(),
        total.0
    );
}
