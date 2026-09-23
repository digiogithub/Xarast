//! Writes the synthetic seed corpora of the two `.xarast` fuzz targets.
//!
//! ```text
//! cargo run -p xarast-format --example fuzz_seeds -- fuzz/corpus
//! ```
//!
//! Everything is generated here from scratch; no real document is involved.

use std::io::Cursor;
use std::path::Path;

use xarast_format::manifest::Role;
use xarast_format::{PackageWriter, ResourceIndex, ResourceKind, WriteOptions};

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    v
}

fn package(f: impl FnOnce(&mut PackageWriter)) -> Vec<u8> {
    let mut w = PackageWriter::new(WriteOptions::deterministic());
    w.set_meta(&b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<meta/>\n"[..]);
    w.set_document(&b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n"[..]);
    f(&mut w);
    let mut out = Cursor::new(Vec::new());
    w.finish(&mut out).expect("seed package");
    out.into_inner()
}

fn main() -> std::io::Result<()> {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fuzz/corpus".into());
    let open = Path::new(&root).join("fuzz_xarast_open");
    let manifest = Path::new(&root).join("fuzz_xarast_manifest");
    std::fs::create_dir_all(&open)?;
    std::fs::create_dir_all(&manifest)?;

    let seeds = [
        ("minimal.xarast", package(|_| {})),
        (
            "thumbnail-and-resources.xarast",
            package(|w| {
                w.set_thumbnail(png(16, 12)).expect("thumbnail");
                let mut ix = ResourceIndex::new();
                ix.insert(ResourceKind::Image, "png", png(4, 4))
                    .expect("png");
                ix.insert(ResourceKind::Blob, "bin", b"text-like ".repeat(64))
                    .expect("bin");
                w.add_resources(&ix);
            }),
        ),
        (
            "unknown-entries.xarast",
            package(|w| {
                w.add_entry(
                    "extensions/acme/x.bin",
                    "application/x-acme",
                    Role::Other("acme".into()),
                    &b"acme"[..],
                )
                .expect("extension");
                w.add_entry(
                    "history/index.xml",
                    "application/xml",
                    Role::History,
                    &b"<h/>"[..],
                )
                .expect("history");
            }),
        ),
    ];
    for (name, bytes) in &seeds {
        std::fs::write(open.join(name), bytes)?;
    }

    // Manifests: ours, the specification's example shape, and one full of
    // foreign data.
    let ours = xarast_format::XarastReader::open(Cursor::new(&seeds[1].1[..]))
        .expect("reopen")
        .manifest()
        .to_xml()
        .expect("manifest");
    std::fs::write(manifest.join("ours.xml"), ours)?;
    std::fs::write(
        manifest.join("foreign.xml"),
        r#"<?xml version="1.0"?>
<m:manifest xmlns:m="https://xarast.org/ns/manifest/1.0" xmlns:acme="urn:acme" xmlns="urn:d"
    m:version="1.3" m:min-reader="1.0" m:profile="portable" acme:build="42" m:future="x">
  <m:requires><m:capability m:name="zstd" m:optional="true"/><acme:note>hi</acme:note></m:requires>
  <m:file-entry m:full-path="/" m:media-type="application/vnd.xarast+zip"/>
  <m:file-entry m:full-path="extensions/a" m:media-type="a/b" m:role="future" acme:s="&amp;">
    <acme:sig alg="x"><inner xmlns="urn:i">t&lt;<![CDATA[raw]]></inner></acme:sig>
  </m:file-entry>
  <acme:root-note xml:lang="en"/>
  <!-- comment --><?pi data?>
</m:manifest>
"#,
    )?;
    std::fs::write(
        manifest.join("digests.xml"),
        r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"><mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip"/><mf:file-entry mf:full-path="resources/images/b3-0123456789abcdef0123456789abcdef.png" mf:media-type="image/png" mf:role="resource" mf:size="10" mf:method="stored" mf:digest="blake3-256" mf:digest-value="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" mf:refcount="3" mf:derived-from="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" mf:derivation="crop 1 2 3 4"/></mf:manifest>"#,
    )?;
    Ok(())
}
