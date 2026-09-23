//! `.xar → .xarast → reload → .xarast`, reporting where the two saves
//! first differ. A development aid for the reader; the gate is
//! `tests/svg_roundtrip.rs`.
//!
//! ```text
//! cargo run --release -p xarast-format --example roundtrip -- FILE.xar…
//! ```

use std::io::Cursor;

use xarast_format::{SaveOptions, WriteOptions, open_reader, save_opened_to, save_to};

fn main() {
    let opts = SaveOptions {
        write: WriteOptions::deterministic(),
        ..SaveOptions::default()
    };
    let mut failures = 0usize;
    let mut paths: Vec<String> = Vec::new();
    for a in std::env::args().skip(1) {
        let p = std::path::Path::new(&a);
        if p.is_dir() {
            let mut v: Vec<String> = std::fs::read_dir(p)
                .expect("dir")
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xar")))
                .map(|p| p.display().to_string())
                .collect();
            v.sort();
            paths.extend(v);
        } else {
            paths.push(a);
        }
    }
    for path in paths {
        let bytes = std::fs::read(&path).expect("read");
        let (doc, _) =
            xarast_xar::import(&bytes, &xarast_xar::ImportOptions::default()).expect("import");
        let mut first = Cursor::new(Vec::new());
        save_to(&doc, &mut first, &opts).expect("save 1");
        let first = first.into_inner();
        let t = std::time::Instant::now();
        let mut opened = match open_reader(Cursor::new(first.clone()), &Default::default()) {
            Ok(o) => o,
            Err(e) => {
                println!("{path}: OPEN FAILED: {e}");
                failures += 1;
                continue;
            }
        };
        let open_time = t.elapsed();
        let mut second = Cursor::new(Vec::new());
        save_opened_to(&opened.document, &mut opened.package, &mut second, &opts).expect("save 2");
        let second = second.into_inner();
        let third = {
            let mut o =
                open_reader(Cursor::new(second.clone()), &Default::default()).expect("reopen");
            let mut third = Cursor::new(Vec::new());
            save_opened_to(&o.document, &mut o.package, &mut third, &opts).expect("save 3");
            third.into_inner()
        };
        if third != second {
            failures += 1;
            println!("{path}: NOT A FIXED POINT on the second re-save");
        }
        let nf_a = xarast_format::svg::normal_form(&doc);
        let nf_b = xarast_format::svg::normal_form(&opened.document);
        let svg = |b: &[u8]| {
            let mut r = xarast_format::XarastReader::open(Cursor::new(b.to_vec())).unwrap();
            String::from_utf8(r.document_bytes().unwrap()).unwrap()
        };
        let (a, b) = (svg(&first), svg(&second));
        let same_bytes = first == second;
        let nf_same = nf_a == nf_b;
        println!(
            "{path}: bytes {} svg {} nf {} open {:?} diags {} stats {:?}",
            if same_bytes { "SAME" } else { "DIFF" },
            if a == b { "same" } else { "diff" },
            if nf_same { "same" } else { "DIFF" },
            open_time,
            opened.diagnostics.len(),
            opened.stats,
        );
        for d in opened.diagnostics.iter().take(5) {
            println!("   diag: {d:?}");
        }
        if a == b && !same_bytes {
            let mut ra = xarast_format::XarastReader::open(Cursor::new(first.clone())).unwrap();
            let mut rb = xarast_format::XarastReader::open(Cursor::new(second.clone())).unwrap();
            let na: Vec<String> = ra.entries().iter().map(|e| e.name.clone()).collect();
            let nb: Vec<String> = rb.entries().iter().map(|e| e.name.clone()).collect();
            if na != nb {
                println!("   entries differ: {na:?} vs {nb:?}");
            }
            for n in &na {
                if let (Ok(x), Ok(y)) = (ra.entry(n), rb.entry(n))
                    && x != y
                {
                    println!("   entry {n} differs ({} vs {} bytes)", x.len(), y.len());
                    if n.ends_with(".xml") {
                        let (x, y) = (String::from_utf8_lossy(&x), String::from_utf8_lossy(&y));
                        for (l1, l2) in x.lines().zip(y.lines()) {
                            if l1 != l2 {
                                println!("   - {l1}\n   + {l2}");
                            }
                        }
                    }
                }
            }
        }
        if a != b {
            failures += 1;
            let pos = a
                .bytes()
                .zip(b.bytes())
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(b.len()));
            let lo = pos.saturating_sub(300);
            let ctx = |s: &str| s.get(lo..(pos + 300).min(s.len())).unwrap_or("").to_owned();
            println!("   first:  {}", ctx(&a).replace('\n', " | "));
            println!("   second: {}", ctx(&b).replace('\n', " | "));
        }
        if !nf_same {
            failures += 1;
            let la: Vec<&str> = nf_a.lines().collect();
            let lb: Vec<&str> = nf_b.lines().collect();
            if let Some(i) = la.iter().zip(&lb).position(|(x, y)| x != y) {
                println!("   nf line {i}:\n     {}\n     {}", la[i], lb[i]);
            } else {
                println!("   nf lengths {} vs {}", la.len(), lb.len());
            }
        }
    }
    println!("failures: {failures}");
}
