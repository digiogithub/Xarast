//! `XarastReader::open` against arbitrary bytes, then a full re-save.
//!
//! Invariants:
//!
//! 1. **Never panic**, whatever the central directory, the local headers or
//!    the manifest claim.
//! 2. **Bounded allocation.** `Limits::FUZZ` caps entries, totals and the
//!    manifest; reads are capped at declared sizes, so nothing a 1 MiB input
//!    declares can cost much more than 1 MiB of decompressed data per entry.
//! 3. **Whatever opens, re-saves into something that opens.** Every package
//!    the reader accepts is carried through `PackageWriter` (raw copies,
//!    resource index, manifest regeneration) and the result must reopen with
//!    no error and with every carried entry intact.

#![no_main]

use std::io::{Cursor, Read};

use libfuzzer_sys::fuzz_target;
use xarast_format::manifest::Manifest;
use xarast_format::{Limits, PackageWriter, ResourceIndex, WriteOptions, XarastReader};

fuzz_target!(|data: &[u8]| {
    let limits = Limits::FUZZ;
    let Ok(mut r) = XarastReader::open_with(Cursor::new(data), limits) else {
        return;
    };

    // The manifest round-trips through its own writer.
    if let Ok(xml) = r.manifest().to_xml() {
        let again = Manifest::parse(xml.as_bytes(), &Limits::DEFAULT)
            .expect("a manifest we wrote must parse");
        assert_eq!(again.entries.len(), r.manifest().entries.len());
    }

    // Every entry, both read paths. Errors are fine; panics are not.
    let names: Vec<String> = r.entries().iter().map(|e| e.name.clone()).collect();
    for n in &names {
        let whole = r.entry_unverified(n);
        if let Ok(mut s) = r.entry_stream(n) {
            let mut buf = Vec::new();
            let streamed = s.read_to_end(&mut buf);
            if let (Ok(w), Ok(_)) = (&whole, &streamed) {
                assert_eq!(w, &buf, "streamed and whole reads disagree");
            }
        }
    }
    let _ = r.verify_all();
    let _ = r.thumbnail();

    // Re-save.
    let (Ok(meta), Ok(doc)) = (r.meta_bytes(), r.document_bytes()) else {
        return;
    };
    let ix = ResourceIndex::from_package(&r);
    let mut w = PackageWriter::new(WriteOptions::deterministic());
    w.set_meta(meta.clone());
    w.set_document(doc.clone());
    w.add_resources(&ix);
    w.carry_from(&r);
    let mut out = Cursor::new(Vec::new());
    // A raw copy can still fail on a lying local header; that is an error,
    // not a finding.
    if w.finish_with_source(&mut out, Some(&mut r)).is_err() {
        return;
    }
    let out = out.into_inner();
    let relaxed = Limits {
        // The manifest we write can be larger than the one we read.
        max_manifest_size: 64 << 20,
        ..limits
    };
    let mut r2 = XarastReader::open_with(Cursor::new(&out[..]), relaxed)
        .expect("a package we wrote must reopen");
    assert_eq!(r2.meta_bytes().expect("meta"), meta);
    assert_eq!(r2.document_bytes().expect("document"), doc);
    for rec in ix.records().filter(|rec| rec.is_live()) {
        assert!(r2.contains(&rec.path()), "resource lost on re-save");
    }
});
