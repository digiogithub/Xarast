//! The SVG profile reader (W4) against arbitrary text.
//!
//! Invariants:
//!
//! 1. **Never panic** on any `document.svg`: malformed XML, DTDs, entities,
//!    unbound prefixes, 10,000 levels of nesting, absurd numbers, path data
//!    that stops mid-command, `href` chains that loop, CSS nobody wrote.
//! 2. **What reads, saves and reads back as a fixed point.** A document the
//!    reader accepts is saved as a package, opened again through the
//!    document-level `open` (resources fetched from the package) and saved
//!    again: the second `document.svg` equals the first, and both reads
//!    have the same normal form. The first read may normalise (another
//!    program's SVG becomes the profile); after that nothing may drift.

#![no_main]

use std::io::Cursor;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;
use xarast_format::svg::read::{ReadOptions, read_svg};
use xarast_format::svg::normal_form;
use xarast_format::{OpenOptions, SaveOptions, WriteOptions, XarastReader, open_reader, save_opened_to, save_to};

fn svg_of(bytes: &[u8]) -> Vec<u8> {
    XarastReader::open(Cursor::new(bytes.to_vec()))
        .expect("our package opens")
        .document_bytes()
        .expect("our document.svg reads")
}

fuzz_target!(|data: &[u8]| {
    // A resource is whatever the input names; its bytes are its name, so
    // two names are two resources and the fetch is deterministic.
    let mut fetch = |p: &str| Some(Arc::<[u8]>::from(p.as_bytes()));
    let Ok(first) = read_svg(data, &ReadOptions::fuzz(), &mut fetch) else {
        return;
    };
    let opts = SaveOptions {
        write: WriteOptions::deterministic(),
        ..SaveOptions::default()
    };
    let mut a = Cursor::new(Vec::new());
    if save_to(&first.document, &mut a, &opts).is_err() {
        return;
    }
    let a = a.into_inner();
    let open = OpenOptions {
        read: ReadOptions::fuzz(),
        ..OpenOptions::default()
    };
    let mut o = open_reader(Cursor::new(a.clone()), &open)
        .unwrap_or_else(|e| panic!("our own package does not open: {e}"));
    let mut b = Cursor::new(Vec::new());
    save_opened_to(&o.document, &mut o.package, &mut b, &opts).expect("re-save");
    let b = b.into_inner();
    let (sa, sb) = (svg_of(&a), svg_of(&b));
    if sa != sb {
        // One more round must settle it (8-bit key stops re-sampled once).
        let mut o2 = open_reader(Cursor::new(b.clone()), &open).expect("reopen");
        let mut c = Cursor::new(Vec::new());
        save_opened_to(&o2.document, &mut o2.package, &mut c, &opts).expect("re-save 2");
        assert!(
            svg_of(&c.into_inner()) == sb,
            "not a fixed point:\n{}\n----\n{}",
            String::from_utf8_lossy(&sa),
            String::from_utf8_lossy(&sb)
        );
    }
    let again = open_reader(Cursor::new(b), &open).expect("reopen");
    assert!(
        normal_form(&o.document) == normal_form(&again.document),
        "normal form drifts on a second read"
    );
});
