//! Times the application's save path on real files: what the interface
//! thread pays (the snapshot) and what the save thread pays (restore,
//! thumbnail, write), as File › Save runs it.
//!
//! ```text
//! cargo run --release -p xarast-app --example save_probe -- FILE.xar|FILE.xarast… OUT_DIR
//! ```

use std::path::PathBuf;
use std::time::Instant;

use xarast_app::save::SaveKind;
use xarast_app::{DocumentId, Session};

fn main() {
    let mut args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let Some(out) = args.pop().filter(|_| !args.is_empty()) else {
        eprintln!("usage: save_probe FILE… OUT_DIR");
        std::process::exit(2);
    };
    std::fs::create_dir_all(&out).expect("output directory");
    for file in args {
        let t = Instant::now();
        let s = Session::open(DocumentId(1), &file).expect("opens");
        let open = t.elapsed();
        let name = file.file_stem().unwrap_or_default().to_string_lossy();
        let target = out.join(format!("{name}.xarast"));
        for (label, thumb) in [("with thumbnail", true), ("no thumbnail", false)] {
            let t = Instant::now();
            let job = s.save_job(SaveKind::Document, &target).expect("job");
            let snapshot = t.elapsed();
            let job = if thumb { job } else { job.without_thumbnail() };
            let t = Instant::now();
            let outcome = job.run();
            let run = t.elapsed();
            let summary = outcome.result.expect("saves");
            println!(
                "{}: {} nodes, open {:.0} ms; {label}: snapshot {:.1} ms (UI thread), \
                 save {:.0} ms (save thread), {} bytes",
                file.display(),
                s.doc.tree.node_count(),
                open.as_secs_f64() * 1e3,
                snapshot.as_secs_f64() * 1e3,
                run.as_secs_f64() * 1e3,
                summary.bytes
            );
        }
        let snap = s.doc.snapshot();
        let t = Instant::now();
        let mut restored = xarast_doc::Document::new_empty();
        restored.restore(&snap);
        println!("  restore alone {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let direct = xarast_format::save(
            &s.doc,
            &out.join("direct.xarast"),
            &xarast_format::SaveOptions::default(),
        )
        .expect("direct save");
        println!(
            "  xarast_format::save alone {:.0} ms (serialise {:.0} ms, package {:.0} ms)",
            t.elapsed().as_secs_f64() * 1e3,
            direct.serialise.as_secs_f64() * 1e3,
            direct.package_time.as_secs_f64() * 1e3
        );
        let t = Instant::now();
        let png = xarast_app::thumbnail::thumbnail_png(&s.doc);
        println!(
            "  thumbnail alone {:.0} ms ({} bytes)",
            t.elapsed().as_secs_f64() * 1e3,
            png.map_or(0, |p| p.len())
        );
    }
}
