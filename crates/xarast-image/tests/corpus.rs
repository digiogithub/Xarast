//! Every bitmap in the 59-file `.xar` corpus, through the façade.
//!
//! The corpus is read in place through `XARAST_XAR_CORPUS` (default
//! `/home/user/xara-xtreme`) and never copied into this repository. Without
//! it the test prints a notice and passes, unless `XARAST_CORPUS_REQUIRED=1`.
//!
//! `cargo test -p xarast-image --test corpus -- --nocapture` prints the
//! per-format table recorded in `docs/memory/image.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use xarast_image::{DecodeLimits, xar};
use xarast_xar::{Decoded, DiagSink, ReaderLimits, RecordReader};

const DIRS: [&str; 4] = ["testfiles", "Designs", "Templates", "TextDesigns"];

fn corpus_files() -> Option<Vec<PathBuf>> {
    let required = std::env::var("XARAST_CORPUS_REQUIRED").as_deref() == Ok("1");
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    let mut files = Vec::new();
    for d in DIRS {
        let Ok(rd) = std::fs::read_dir(root.join(d)) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("xar") {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.len() != 59 {
        assert!(
            !required,
            "XARAST_CORPUS_REQUIRED=1 but {} holds {} .xar files, not 59",
            root.display(),
            files.len()
        );
        return None;
    }
    Some(files)
}

#[derive(Default)]
struct Row {
    records: u32,
    decoded: u32,
    failed: u32,
    pixels: u64,
    translucent: u32,
    micros: u128,
}

fn label(tag: u32, sniffed: Option<xarast_image::ImageFormat>) -> String {
    let kind = match tag {
        60..=64 => "preview",
        65 => "DEFINEBITMAP_BMP",
        66 => "DEFINEBITMAP_GIF",
        67 => "DEFINEBITMAP_JPEG",
        68 => "DEFINEBITMAP_PNG",
        69 => "DEFINEBITMAP_BMPZIP",
        71 => "DEFINEBITMAP_JPEG8BPP",
        _ => "?",
    };
    let f = sniffed.map_or("(no magic)", |f| f.name());
    format!("{tag} {kind} [{f}]")
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_owned()
}

#[test]
fn every_corpus_bitmap_decodes() {
    let Some(files) = corpus_files() else {
        println!(
            "skipping: no .xar corpus (set XARAST_XAR_CORPUS; \
             XARAST_CORPUS_REQUIRED=1 to make this a failure)"
        );
        return;
    };
    let limits = DecodeLimits::default();
    let mut table: BTreeMap<String, Row> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut files_with_bitmaps = 0;
    for path in &files {
        let bytes = std::fs::read(path).expect("read corpus file");
        let Ok(mut reader) = RecordReader::new(&bytes, ReaderLimits::default()) else {
            continue;
        };
        let mut any = false;
        while let Some(rec) = reader.next_record() {
            let Ok(rec) = rec else { break };
            if xar::XarWrapping::from_tag(rec.tag).is_none() {
                continue;
            }
            let (image, palette) = if (60..=64).contains(&rec.tag) {
                (&rec.data[..], Vec::new())
            } else {
                let mut diags = DiagSink::new();
                let Ok(Decoded::BitmapDefinition(def)) = xarast_xar::decode(
                    rec.tag,
                    &rec.data,
                    xarast_geom::Point::ORIGIN,
                    &mut diags,
                    (rec.number, rec.tag),
                ) else {
                    failures.push(format!(
                        "{}: record {} undecodable",
                        name_of(path),
                        rec.number
                    ));
                    continue;
                };
                any = true;
                (
                    rec.data.get(def.image.clone()).unwrap_or(&[]),
                    def.palette.clone(),
                )
            };
            let row = table
                .entry(label(rec.tag, xarast_image::sniff(image)))
                .or_default();
            row.records += 1;
            let t = Instant::now();
            match xar::decode_xar_bitmap(rec.tag, image, &palette, &limits) {
                Ok(d) => {
                    row.micros += t.elapsed().as_micros();
                    row.decoded += 1;
                    if let Some(f) = xarast_image::sniff(image) {
                        let p = xarast_image::probe_as(image, f).expect("probe");
                        assert_eq!(
                            (p.info.pixel_width, p.info.pixel_height),
                            (d.data.width, d.data.height),
                            "{}: record {}: probe and decode disagree",
                            name_of(path),
                            rec.number
                        );
                    }
                    row.pixels += u64::from(d.data.width) * u64::from(d.data.height);
                    row.translucent += u32::from(d.info.has_alpha);
                    assert_eq!(
                        d.data.pixels.len(),
                        d.data.width as usize * d.data.height as usize * 4
                    );
                    assert!(
                        d.data
                            .pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|p| p[..3].iter().all(|c| *c <= p[3])),
                        "{}: record {} is not premultiplied",
                        name_of(path),
                        rec.number
                    );
                }
                Err(e) => {
                    row.failed += 1;
                    failures.push(format!(
                        "{}: record {} tag {}: {e}",
                        name_of(path),
                        rec.number,
                        rec.tag
                    ));
                }
            }
        }
        files_with_bitmaps += usize::from(any);
    }
    println!("| Record kind [sniffed] | Records | Decoded | Failed | Mpx | With alpha | ms |");
    println!("|---|---|---|---|---|---|---|");
    for (k, r) in &table {
        println!(
            "| {k} | {} | {} | {} | {:.2} | {} | {:.1} |",
            r.records,
            r.decoded,
            r.failed,
            r.pixels as f64 / 1e6,
            r.translucent,
            r.micros as f64 / 1000.0
        );
    }
    println!(
        "files with bitmap definitions: {files_with_bitmaps} of {}",
        files.len()
    );
    for f in &failures {
        println!("FAIL {f}");
    }
    assert!(
        failures.is_empty(),
        "{} corpus bitmaps failed",
        failures.len()
    );
    assert!(table.values().map(|r| r.decoded).sum::<u32>() > 0);
}
