//! The container end to end: writer → bytes → reader, and hostile packages.

use std::io::{Cursor, Write};
use std::sync::Arc;

use xarast_format::digest::Digest;
use xarast_format::error::{Diagnostic, ReadError};
use xarast_format::manifest::{EntryDigest, FileEntry, Manifest, Role};
use xarast_format::name::NameError;
use xarast_format::resource::{ResourceId, ResourceIndex, ResourceKind};
use xarast_format::{
    Limits, MIME_TYPE, Method, PackageWriter, Profile, WriteOptions, XarastReader, sniff_bytes,
};
use zip::write::SimpleFileOptions;

const SVG: &[u8] =
    b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n";
const META: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<meta/>\n";

/// A PNG header good enough for the writer's check (it never decodes).
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    v
}

fn minimal(opts: WriteOptions) -> PackageWriter {
    let mut w = PackageWriter::new(opts);
    w.set_meta(META);
    w.set_document(SVG);
    w
}

fn write(w: PackageWriter) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    w.finish(&mut out).unwrap();
    out.into_inner()
}

fn open(bytes: &[u8]) -> XarastReader<Cursor<&[u8]>> {
    XarastReader::open(Cursor::new(bytes)).unwrap()
}

fn open_err(bytes: &[u8]) -> ReadError {
    XarastReader::open(Cursor::new(bytes)).unwrap_err()
}

#[test]
fn magic_bytes_at_fixed_offsets() {
    let z = write(minimal(WriteOptions::default()));
    assert_eq!(&z[0..4], b"PK\x03\x04");
    assert_eq!(&z[8..10], &[0, 0], "mimetype is STORED");
    assert_eq!(&z[28..30], &[0, 0], "no extra field");
    assert_eq!(&z[30..38], b"mimetype");
    assert_eq!(&z[38..64], MIME_TYPE.as_bytes());
    assert!(sniff_bytes(&z));
}

#[test]
fn round_trip_with_every_kind_of_entry() {
    let mut ix = ResourceIndex::new();
    let jpeg: Arc<[u8]> = b"\xff\xd8\xff\xe0 pretend jpeg".repeat(100).into();
    let icc: Arc<[u8]> = b"icc profile table ".repeat(500).into();
    let jid = ix.insert(ResourceKind::Image, "jpg", jpeg.clone()).unwrap();
    let iid = ix
        .insert(ResourceKind::Profile, "icc", icc.clone())
        .unwrap();

    let mut w = minimal(WriteOptions::deterministic());
    w.set_thumbnail(png(256, 180)).unwrap();
    w.set_preview(1, png(512, 360)).unwrap();
    w.add_entry(
        "extensions/acme/data.bin",
        "application/octet-stream",
        Role::Extension,
        &b"acme"[..],
    )
    .unwrap();
    w.add_resources(&ix);
    let z = write(w);

    let mut r = open(&z);
    assert_eq!(r.diagnostics(), &[], "a fresh package is conformant");
    assert_eq!(r.profile(), Profile::Portable);
    assert_eq!(r.format_version().to_string(), "1.0");
    let names: Vec<&str> = r.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "mimetype",
            "META-INF/manifest.xml",
            "meta.xml",
            "document.svg",
            "thumbnail.png",
            "previews/spread-1.png",
            &*format!("resources/images/b3-{}.jpg", jid.short_hex()),
            &*format!("resources/profiles/b3-{}.icc", iid.short_hex()),
            "extensions/acme/data.bin",
        ]
    );
    // One manifest row per entry, and the `/` row.
    assert_eq!(r.manifest().entries.len(), names.len());
    assert_eq!(r.manifest().root_rows, 1);
    assert_eq!(r.document_bytes().unwrap(), SVG);
    assert_eq!(r.meta_bytes().unwrap(), META);
    assert_eq!(r.thumbnail().unwrap().unwrap(), png(256, 180));
    assert_eq!(r.preview(1).unwrap().unwrap(), png(512, 360));
    assert_eq!(r.preview(2).unwrap(), None);
    assert_eq!(&r.resource(jid).unwrap()[..], &jpeg[..]);
    assert_eq!(&r.resource(iid).unwrap()[..], &icc[..]);
    assert!(r.verify_all().is_empty());
    let row = r.manifest().entry("document.svg").unwrap();
    assert_eq!(row.blake3(), Some(Digest::of(SVG)));
    assert_eq!(row.size, Some(SVG.len() as u64));
}

#[test]
fn open_does_not_parse_the_document() {
    let mut w = PackageWriter::new(WriteOptions::default());
    w.set_meta(META);
    w.set_document(&b"this is not XML at all <<<"[..]);
    let z = write(w);
    let mut r = open(&z);
    assert!(r.diagnostics().is_empty());
    assert_eq!(r.document_bytes().unwrap(), b"this is not XML at all <<<");
}

#[test]
fn deterministic_writes_are_byte_identical() {
    let build = || {
        let mut ix = ResourceIndex::new();
        ix.insert(ResourceKind::Image, "png", &png(4, 4)[..])
            .unwrap();
        let mut w = minimal(WriteOptions::deterministic());
        w.add_resources(&ix);
        write(w)
    };
    assert_eq!(build(), build());
}

/// Open, carry everything over (resources raw-copied from the package) and
/// save again: the first re-save is a fixed point.
#[test]
fn resave_from_the_package_is_a_fixed_point() {
    let mut ix = ResourceIndex::new();
    ix.insert(ResourceKind::Image, "png", &png(8, 8)[..])
        .unwrap();
    ix.insert(ResourceKind::Blob, "bin", &b"opaque".repeat(1000)[..])
        .unwrap();
    let mut w = minimal(WriteOptions::deterministic());
    w.add_entry(
        "history/index.xml",
        "application/xml",
        Role::History,
        &b"<h/>"[..],
    )
    .unwrap();
    w.add_entry(
        "zzz-unknown.dat",
        "application/octet-stream",
        Role::Unknown,
        &b"?"[..],
    )
    .unwrap();
    w.add_resources(&ix);
    let first = write(w);

    let resave = |bytes: &[u8]| {
        let mut r = open(bytes);
        let ix = ResourceIndex::from_package(&r);
        assert_eq!(ix.len(), 2);
        let mut w = PackageWriter::new(WriteOptions::deterministic());
        w.set_meta(r.meta_bytes().unwrap());
        w.set_document(r.document_bytes().unwrap());
        w.add_resources(&ix);
        w.carry_from(&r);
        let mut out = Cursor::new(Vec::new());
        let rep = w.finish_with_source(&mut out, Some(&mut r)).unwrap();
        assert!(
            rep.entries
                .iter()
                .filter(|e| e.name.starts_with("resources/"))
                .all(|e| e.raw_copy)
        );
        out.into_inner()
    };
    let second = resave(&first);
    assert_eq!(
        first, second,
        "re-save of an unchanged package is byte-identical"
    );
}

#[test]
fn eight_uses_one_entry() {
    let image: Arc<[u8]> = png(64, 64).repeat(200).into();
    let mut ix = ResourceIndex::new();
    for _ in 0..8 {
        ix.insert(ResourceKind::Image, "png", image.clone())
            .unwrap();
    }
    let mut w = minimal(WriteOptions::default());
    w.add_resources(&ix);
    let mut out = Cursor::new(Vec::new());
    let rep = w.finish(&mut out).unwrap();
    assert_eq!(rep.entries_under("resources/images/"), 1);
    assert!(rep.bytes_saved_by_dedup > 7 * image.len() as u64 - 4096);
    let r = open(out.get_ref());
    let row = r
        .manifest()
        .entries
        .iter()
        .find(|e| e.role == Some(Role::Resource))
        .unwrap();
    assert_eq!(row.refcount, Some(8));
}

#[test]
fn compression_policy() {
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let noise: Vec<u8> = (0..300_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let mut ix = ResourceIndex::new();
    let jpg = ix
        .insert(ResourceKind::Image, "jpg", &b"jpeg-ish ".repeat(1000)[..])
        .unwrap();
    let pngr = ix
        .insert(ResourceKind::Image, "png", &b"png-ish ".repeat(1000)[..])
        .unwrap();
    let webp = ix
        .insert(ResourceKind::Image, "webp", &b"webp-ish ".repeat(1000)[..])
        .unwrap();
    let font = ix
        .insert(ResourceKind::Font, "woff2", &b"woff2-ish ".repeat(1000)[..])
        .unwrap();
    let blob = ix.insert(ResourceKind::Blob, "bin", noise).unwrap();
    let text_blob = ix
        .insert(ResourceKind::Blob, "bin", &b"text-like ".repeat(1000)[..])
        .unwrap();
    let mut w = minimal(WriteOptions::default());
    w.add_resources(&ix);
    let mut out = Cursor::new(Vec::new());
    let rep = w.finish(&mut out).unwrap();
    let m = |id: ResourceId| {
        let rec = ix.get(id).unwrap();
        rep.method_of(&rec.path()).unwrap()
    };
    // Already compressed: never recompressed, however compressible the bytes.
    for id in [jpg, pngr, webp, font] {
        assert_eq!(m(id), Method::Stored);
    }
    assert_eq!(m(blob), Method::Stored, "the 64 KiB heuristic stores noise");
    assert_eq!(m(text_blob), Method::Deflate);
    assert_eq!(rep.method_of("document.svg"), Some(Method::Deflate));
    assert_eq!(rep.method_of("meta.xml"), Some(Method::Deflate));
    assert_eq!(
        rep.method_of("META-INF/manifest.xml"),
        Some(Method::Deflate)
    );
    assert_eq!(rep.method_of("mimetype"), Some(Method::Stored));
}

#[test]
fn gc_drops_unreferenced_resources_from_the_package() {
    let mut ix = ResourceIndex::new();
    let keep = ix.insert(ResourceKind::Image, "png", &b"keep"[..]).unwrap();
    let drop = ix.insert(ResourceKind::Image, "png", &b"drop"[..]).unwrap();
    ix.release(drop);
    let mut w = minimal(WriteOptions::default());
    w.add_resources(&ix);
    let z = write(w);
    let r = open(&z);
    assert!(r.contains(&ix.get(keep).unwrap().path()));
    assert!(!r.contains(&ix.get(drop).unwrap().path()));
}

#[test]
fn writer_refuses_bad_input() {
    let mut w = PackageWriter::new(WriteOptions::default());
    assert!(
        w.add_entry("../evil", "a/b", Role::Unknown, &b""[..])
            .is_err()
    );
    assert!(
        w.add_entry("document.svg", "a/b", Role::Unknown, &b""[..])
            .is_err()
    );
    assert!(
        w.add_entry("resources/images/x.png", "a/b", Role::Unknown, &b""[..])
            .is_err()
    );
    assert!(
        w.add_entry("META-INF/manifest.xml", "a/b", Role::Unknown, &b""[..])
            .is_err()
    );
    assert!(w.set_thumbnail(png(600, 10)).is_err());
    assert!(w.set_preview(0, png(10, 10)).is_err());
    assert!(
        w.clone().finish(Cursor::new(Vec::new())).is_err(),
        "meta and document are mandatory"
    );
    w.set_meta(META);
    assert!(w.finish(Cursor::new(Vec::new())).is_err());
}

// ───────────────────────────────────────────── hand-built packages

/// A ZIP written directly with the `zip` crate, `mimetype` first unless the
/// caller puts something else first.
fn raw_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        z.start_file(*name, stored).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn manifest_for(rows: &[(&str, &str, Role, Option<&[u8]>)]) -> Vec<u8> {
    let mut m = Manifest::new(Profile::Portable, None);
    let mut mt = FileEntry::new("mimetype", "text/plain");
    mt.role = Some(Role::Mimetype);
    m.entries.push(mt);
    let mut mf = FileEntry::new("META-INF/manifest.xml", "application/xml");
    mf.role = Some(Role::Manifest);
    m.entries.push(mf);
    for (path, mt, role, data) in rows {
        let mut e = FileEntry::new(*path, *mt);
        e.role = Some(role.clone());
        if let Some(d) = data {
            e.digest = Some(EntryDigest::Blake3(Digest::of(d)));
            e.size = Some(d.len() as u64);
        }
        m.entries.push(e);
    }
    m.to_xml().unwrap().into_bytes()
}

fn conformant_rows() -> Vec<(&'static str, &'static str, Role, Option<&'static [u8]>)> {
    vec![
        ("meta.xml", "application/xml", Role::Meta, Some(META)),
        ("document.svg", "image/svg+xml", Role::Document, Some(SVG)),
    ]
}

#[test]
fn hand_built_conformant_package_opens_clean() {
    let mf = manifest_for(&conformant_rows());
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("meta.xml", META),
        ("document.svg", SVG),
    ]);
    assert_eq!(open(&z).diagnostics(), &[]);
}

#[test]
fn signature_and_mimetype_rules() {
    let mf = manifest_for(&conformant_rows());
    // Not first.
    let z = raw_zip(&[
        ("meta.xml", META),
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("document.svg", SVG),
    ]);
    assert!(matches!(open_err(&z), ReadError::NotXarast));
    // Deflated.
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zw.start_file("mimetype", SimpleFileOptions::default())
        .unwrap();
    zw.write_all(MIME_TYPE.as_bytes()).unwrap();
    assert!(matches!(
        open_err(&zw.finish().unwrap().into_inner()),
        ReadError::NotXarast
    ));
    // A plain ZIP.
    assert!(matches!(
        open_err(&raw_zip(&[("a", b"b")])),
        ReadError::NotXarast
    ));
    assert!(matches!(open_err(b""), ReadError::NotXarast));
}

#[test]
fn missing_mandatory_entries() {
    let mf = manifest_for(&conformant_rows());
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("meta.xml", META),
        ("document.svg", SVG),
    ]);
    assert!(matches!(
        open_err(&z),
        ReadError::MissingEntry("META-INF/manifest.xml")
    ));
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("meta.xml", META),
    ]);
    assert!(matches!(
        open_err(&z),
        ReadError::MissingEntry("document.svg")
    ));
}

#[test]
fn zip_slip_names_are_rejected() {
    let mf = manifest_for(&conformant_rows());
    for evil in [
        "../evil",
        "/etc/passwd",
        "a\\b",
        "resources/../../x",
        "C:/x",
        "a//b",
    ] {
        let z = raw_zip(&[
            ("mimetype", MIME_TYPE.as_bytes()),
            ("META-INF/manifest.xml", &mf),
            ("meta.xml", META),
            ("document.svg", SVG),
            (evil, b"x"),
        ]);
        assert!(
            matches!(open_err(&z), ReadError::BadName { .. }),
            "{evil:?}"
        );
    }
}

fn replace_all(hay: &mut [u8], from: &[u8], to: &[u8]) -> usize {
    assert_eq!(from.len(), to.len());
    let mut n = 0;
    let mut i = 0;
    while i + from.len() <= hay.len() {
        if &hay[i..i + from.len()] == from {
            hay[i..i + to.len()].copy_from_slice(to);
            n += 1;
            i += from.len();
        } else {
            i += 1;
        }
    }
    n
}

#[test]
fn duplicate_entries_are_rejected() {
    let mf = manifest_for(&conformant_rows());
    let mut z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("meta.xml", META),
        ("document.svg", SVG),
        ("extensions/aaaa", b"1"),
        ("extensions/bbbb", b"2"),
    ]);
    assert_eq!(
        replace_all(&mut z, b"extensions/bbbb", b"extensions/aaaa"),
        2
    );
    assert!(matches!(open_err(&z), ReadError::DuplicateEntries));
}

#[test]
fn non_ascii_name_without_the_utf8_flag_is_rejected() {
    let mf = manifest_for(&conformant_rows());
    let mut z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("meta.xml", META),
        ("document.svg", SVG),
        ("extensions/é", b"x"),
    ]);
    // Clear bit 11 in both headers of that entry: local header flags at +6,
    // central header flags at +8.
    let name = "extensions/é".as_bytes();
    let mut patched = 0;
    for i in 0..z.len() - name.len() {
        if &z[i..i + name.len()] == name {
            if i >= 30 && &z[i - 30..i - 26] == b"PK\x03\x04" {
                z[i - 30 + 7] &= !0x08;
                patched += 1;
            } else if i >= 46 && &z[i - 46..i - 42] == b"PK\x01\x02" {
                z[i - 46 + 9] &= !0x08;
                patched += 1;
            }
        }
    }
    assert_eq!(patched, 2);
    assert!(matches!(
        open_err(&z),
        ReadError::BadName {
            reason: NameError::MissingUtf8Flag,
            ..
        }
    ));
}

#[test]
fn encrypted_zip_bombs_and_totals_are_rejected() {
    let mf = manifest_for(&conformant_rows());
    // A 10 MB run of zeros deflates ~1000:1.
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (n, d) in [
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf[..]),
        ("meta.xml", META),
        ("document.svg", SVG),
    ] {
        zw.start_file(n, stored).unwrap();
        zw.write_all(d).unwrap();
    }
    zw.start_file("extensions/bomb", SimpleFileOptions::default())
        .unwrap();
    zw.write_all(&vec![0u8; 10 << 20]).unwrap();
    let z = zw.finish().unwrap().into_inner();
    assert!(matches!(open_err(&z), ReadError::ZipBomb { .. }));

    // The same package is fine with the ratio check relaxed, and a total
    // limit below its size refuses it.
    let relaxed = Limits {
        max_entry_ratio: 10_000,
        ..Limits::DEFAULT
    };
    assert!(XarastReader::open_with(Cursor::new(&z[..]), relaxed).is_ok());
    let small = Limits {
        max_total_uncompressed: 1 << 20,
        ..relaxed
    };
    assert!(matches!(
        XarastReader::open_with(Cursor::new(&z[..]), small).unwrap_err(),
        ReadError::TooLarge { .. }
    ));
    let few = Limits {
        max_entries: 3,
        ..relaxed
    };
    assert!(matches!(
        XarastReader::open_with(Cursor::new(&z[..]), few).unwrap_err(),
        ReadError::TooManyEntries { .. }
    ));
}

#[test]
fn manifest_divergence_is_diagnosed_not_fatal() {
    let mut rows = conformant_rows();
    rows.push(("extensions/listed-but-absent", "a/b", Role::Extension, None));
    let mf = manifest_for(&rows);
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("meta.xml", META),
        ("document.svg", SVG),
        ("extensions/unlisted", b"x"),
    ]);
    let r = open(&z);
    let d = r.diagnostics();
    assert!(d.contains(&Diagnostic::UnlistedEntry {
        path: "extensions/unlisted".into()
    }));
    assert!(d.contains(&Diagnostic::MissingEntry {
        path: "extensions/listed-but-absent".into()
    }));
    assert!(
        r.suggests_read_only(),
        "never open silently past a divergence"
    );
}

#[test]
fn digest_mismatch_is_caught_on_read() {
    let rows = vec![
        ("meta.xml", "application/xml", Role::Meta, Some(META)),
        (
            "document.svg",
            "image/svg+xml",
            Role::Document,
            Some(&b"something else"[..]),
        ),
    ];
    let mf = manifest_for(&rows);
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", &mf),
        ("meta.xml", META),
        ("document.svg", SVG),
    ]);
    let mut r = open(&z);
    assert!(matches!(
        r.document_bytes(),
        Err(ReadError::DigestMismatch { .. })
    ));
    assert_eq!(r.entry_unverified("document.svg").unwrap(), SVG);
    let mut s = String::new();
    let err = std::io::Read::read_to_string(&mut r.entry_stream("document.svg").unwrap(), &mut s)
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        r.verify_all(),
        vec![Diagnostic::DigestMismatch {
            path: "document.svg".into()
        }]
    );
}

#[test]
fn newer_min_reader_and_capabilities_are_diagnosed() {
    let mut m = Manifest::parse(&manifest_for(&conformant_rows()), &Limits::DEFAULT).unwrap();
    m.min_reader = xarast_format::Version { major: 1, minor: 7 };
    m.requires.push(xarast_format::manifest::Capability {
        name: "mesh-fill-v2".into(),
        optional: true,
        foreign_attrs: vec![],
        foreign_children: vec![],
    });
    let mf = m.to_xml().unwrap();
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", mf.as_bytes()),
        ("meta.xml", META),
        ("document.svg", SVG),
    ]);
    let r = open(&z);
    assert!(r.diagnostics().contains(&Diagnostic::NewerFormat {
        min_reader: xarast_format::Version { major: 1, minor: 7 }
    }));
    assert!(r.diagnostics().contains(&Diagnostic::MissingCapability {
        name: "mesh-fill-v2".into(),
        optional: true
    }));
    assert!(r.suggests_read_only());

    m.version.major = 2;
    let mf = m.to_xml().unwrap();
    let z = raw_zip(&[
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", mf.as_bytes()),
        ("meta.xml", META),
        ("document.svg", SVG),
    ]);
    assert!(matches!(open_err(&z), ReadError::UnsupportedMajor(_)));
}

#[test]
fn unknown_entries_are_carried_byte_for_byte() {
    let mut m = Manifest::parse(&manifest_for(&conformant_rows()), &Limits::DEFAULT).unwrap();
    let mut row = FileEntry::new("extensions/acme/state.bin", "application/x-acme");
    row.role = Some(Role::Other("acme-state".into()));
    row.foreign_attrs
        .push(xarast_format::manifest::ForeignAttr {
            namespace: "urn:acme".into(),
            local: "rev".into(),
            prefix: Some("acme".into()),
            value: "7".into(),
        });
    m.entries.push(row);
    let mut odd = FileEntry::new("resources/images/not-hash-named.png", "image/png");
    odd.role = Some(Role::Resource);
    m.entries.push(odd);
    let mf = m.to_xml().unwrap();
    // The unknown entry is deflated in the source: it must come back with
    // the same compressed bytes, not recompressed.
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (n, d) in [
        ("mimetype", MIME_TYPE.as_bytes()),
        ("META-INF/manifest.xml", mf.as_bytes()),
        ("meta.xml", META),
        ("document.svg", SVG),
    ] {
        zw.start_file(n, stored).unwrap();
        zw.write_all(d).unwrap();
    }
    zw.start_file(
        "extensions/acme/state.bin",
        SimpleFileOptions::default().compression_level(Some(1)),
    )
    .unwrap();
    zw.write_all(&b"acme state ".repeat(300)).unwrap();
    zw.start_file("resources/images/not-hash-named.png", stored)
        .unwrap();
    zw.write_all(b"odd").unwrap();
    let src = zw.finish().unwrap().into_inner();

    let mut r = open(&src);
    let mut w = PackageWriter::new(WriteOptions::deterministic());
    w.set_meta(r.meta_bytes().unwrap());
    w.set_document(r.document_bytes().unwrap());
    w.carry_from(&r);
    let mut out = Cursor::new(Vec::new());
    let rep = w.finish_with_source(&mut out, Some(&mut r)).unwrap();
    assert!(
        rep.entries
            .iter()
            .any(|e| e.name == "extensions/acme/state.bin" && e.raw_copy)
    );

    let src_info = r.entry_info("extensions/acme/state.bin").unwrap().clone();
    let mut r2 = open(out.get_ref());
    // The source was out of order; the copy is not. Carried rows are carried
    // as they were, warts included: the digest-less resource row stays so.
    assert!(
        r.diagnostics()
            .iter()
            .any(|d| matches!(d, Diagnostic::OutOfOrder { .. }))
    );
    assert_eq!(
        r2.diagnostics(),
        &[Diagnostic::MissingDigest {
            path: "resources/images/not-hash-named.png".into()
        }]
    );
    let info = r2.entry_info("extensions/acme/state.bin").unwrap().clone();
    assert_eq!(
        (info.compressed_size, info.crc32, info.method),
        (src_info.compressed_size, src_info.crc32, src_info.method)
    );
    assert_eq!(
        r2.entry("extensions/acme/state.bin").unwrap(),
        b"acme state ".repeat(300)
    );
    assert_eq!(
        r2.entry("resources/images/not-hash-named.png").unwrap(),
        b"odd"
    );
    let row = r2.manifest().entry("extensions/acme/state.bin").unwrap();
    assert_eq!(row.role, Some(Role::Other("acme-state".into())));
    assert_eq!(row.foreign_attrs[0].value, "7");
}

#[test]
fn zip64_above_65535_entries() {
    let mut w = minimal(WriteOptions::deterministic());
    for i in 0..66_000u32 {
        w.add_entry(
            &format!("extensions/many/{i:05}"),
            "application/octet-stream",
            Role::Extension,
            &b"x"[..],
        )
        .unwrap();
    }
    let z = write(w);
    let limits = Limits {
        max_manifest_size: 64 << 20,
        ..Limits::DEFAULT
    };
    let r = XarastReader::open_with(Cursor::new(&z[..]), limits).unwrap();
    assert_eq!(r.entries().len(), 66_004);
    assert!(r.diagnostics().is_empty());
}

#[test]
fn save_atomic_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("drawing.xarast");
    let rep =
        xarast_format::write_atomic(&path, |f| minimal(WriteOptions::default()).finish(f)).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(rep.bytes_written, bytes.len() as u64);
    let mut f = std::fs::File::open(&path).unwrap();
    assert!(xarast_format::sniff(&mut f).unwrap());
    let mut r = XarastReader::open(f).unwrap();
    assert_eq!(r.document_bytes().unwrap(), SVG);
    let _ = ResourceId::of(b"");
}

/// The exact bytes of a deterministic save. This fails if anything changes
/// the output of a byte-reproducible save: the entry layout, the manifest,
/// the ZIP headers, or the DEFLATE backend (which feature unification can
/// swap under us, see the workspace manifest). If the change is intended,
/// update the digest and say why in the commit.
#[test]
fn deterministic_bytes_are_pinned() {
    let mut opts = WriteOptions::deterministic();
    opts.generator = "Xarast/test".into();
    let mut w = PackageWriter::new(opts);
    w.set_meta(META);
    let rects: String = (0..500)
        .map(|i| {
            format!(
                "<rect x=\"{i}\" y=\"{}\" width=\"10\" height=\"10\"/>\n",
                i * 3 % 97
            )
        })
        .collect();
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\">\n{rects}</svg>\n"
    );
    w.set_document(doc.into_bytes());
    let mut ix = ResourceIndex::new();
    ix.insert(ResourceKind::Blob, "bin", &b"pinned ".repeat(500)[..])
        .unwrap();
    w.add_resources(&ix);
    let z = write(w);
    assert_eq!(
        Digest::of(&z).to_hex(),
        "b4c3284aa212841fd2547bb377820545d3f941b0a7901ac4baa2e449cb716f17",
        "{} bytes",
        z.len()
    );
}

/// The workspace's DEFLATE backend alone (`zlib-rs`, XARA-T-0090), so that a
/// backend swap is told apart from a layout change in the test above. Level
/// 6 is what the container writes; level 1 is the escape heuristic's sample.
#[test]
fn deflate_backend_is_pinned() {
    use flate2::{Compression, write::DeflateEncoder};
    let input: Vec<u8> = (0..20_000u32)
        .flat_map(|i| {
            format!(
                "<path d=\"M{} {}l{} {}z\"/>",
                i % 613,
                i * 7 % 997,
                i % 13,
                i % 29
            )
            .into_bytes()
        })
        .collect();
    let deflate = |level| {
        let mut e = DeflateEncoder::new(Vec::new(), Compression::new(level));
        e.write_all(&input).unwrap();
        e.finish().unwrap()
    };
    let (l6, l1) = (deflate(6), deflate(1));
    assert_eq!(
        (Digest::of(&l6).to_hex(), Digest::of(&l1).to_hex()),
        (
            "3210e5b978f7150e734d38eff1356e95786702d25ceba803ed095b9d3b4b763f".to_owned(),
            "954bb67044b639afc2a1d794d32ccd968f30e230b526bc9c1cf7225c43fe6b4b".to_owned()
        ),
        "the DEFLATE backend changed (expected `zlib-rs`, see the workspace manifest); {} / {} bytes",
        l6.len(),
        l1.len()
    );
}
