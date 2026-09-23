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

/// Seeds of `fuzz_xarast_svg_read`: what the writer makes of a small
/// synthetic document (with and without passes 4–5), and SVG another
/// program could have written — foreign data, CSS, transforms, arcs.
fn svg_seeds(root: &Path) -> std::io::Result<()> {
    use std::sync::Arc;
    use xarast_color::{Colour, ColourDef, ColourValue};
    use xarast_doc::fill::{FillGeometry, Ramp};
    use xarast_doc::{
        AttrValue, BuildLimits, ForeignAttr, ForeignBaggage, ForeignChild, ForeignChildKind,
        ForeignMarks, NodeKind, PathNode, ShapeKind, ShapeNode,
    };
    use xarast_format::svg::{SvgOptions, write_svg};
    use xarast_geom::{BiasGain, Path as GPath, Point, Vector};

    let dir = root.join("fuzz_xarast_svg_read");
    std::fs::create_dir_all(&dir)?;
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).expect("skeleton");
    let red = b.define_colour(ColourDef::normal(ColourValue::rgb(0.8, 0.2, 0.2)).named("Red"));
    for i in 0..10 {
        b.node(NodeKind::Shape(Box::new(ShapeNode {
            shape: if i % 2 == 0 {
                ShapeKind::Rect
            } else {
                ShapeKind::Ellipse
            },
            origin: Point::raw(10_000 * i, 20_000),
            major: Vector::raw(8_000, i * 100),
            minor: Vector::raw(0, 6_000),
        })))
        .expect("shape");
        b.push_scope().expect("scope");
        b.attribute(AttrValue::Fill(FillGeometry::Flat {
            value: Colour::Indexed {
                id: red,
                tint: None,
            },
        }))
        .expect("fill");
        b.pop_scope();
    }
    let mut pb = GPath::builder();
    pb.move_to(Point::raw(0, 0))
        .line_to(Point::raw(50_000, 0))
        .cubic_to(
            Point::raw(60_000, 10_000),
            Point::raw(40_000, 40_000),
            Point::raw(0, 50_000),
        )
        .close();
    let p = b
        .node(NodeKind::Path(Box::new(PathNode::new(pb.build()))))
        .expect("path");
    b.push_scope().expect("scope");
    let mut ramp = Ramp::new();
    ramp.profile = BiasGain::new(0.2, 0.1);
    b.attribute(AttrValue::Fill(FillGeometry::Linear {
        start: Point::raw(0, 0),
        end: Point::raw(50_000, 50_000),
        persp: None,
        from: Colour::Direct(ColourValue::rgb(1.0, 0.0, 0.0)),
        to: Colour::Direct(ColourValue::rgb(0.0, 0.0, 1.0)),
        ramp,
    }))
    .expect("gradient");
    b.pop_scope();
    b.foreign(
        p,
        ForeignBaggage {
            attrs: vec![ForeignAttr {
                ns: Arc::from("urn:acme"),
                prefix: Some(Arc::from("acme")),
                local: Arc::from("state"),
                value: Arc::from("x"),
            }],
            children: vec![ForeignChild {
                position: 0,
                kind: ForeignChildKind::Comment,
                raw: Arc::from("<!-- note -->"),
            }],
            marks: ForeignMarks::DIRTY,
        },
    );
    // A text story with the pieces of the exact twin (XARA-T-0172): text
    // attributes, a kern, an item with its own attributes, an empty line,
    // a soft and a paragraph break, a character XML cannot carry.
    {
        use xarast_doc::{LineSpacing, Script, TextItem, TextStoryNode};
        b.node(NodeKind::TextStory(Box::new(TextStoryNode {
            transform: xarast_geom::Matrix {
                a: 0.8,
                b: 0.6,
                c: -0.6,
                d: 0.8,
                e: xarast_geom::Mp::new(20_000),
                f: xarast_geom::Mp::new(70_000),
            },
            ..TextStoryNode::default()
        })))
        .expect("story");
        b.push_scope().expect("scope");
        b.attribute(AttrValue::FontSize(xarast_geom::Mp::new(12_000)))
            .expect("size");
        b.node(NodeKind::TextLine(Box::default())).expect("line");
        b.push_scope().expect("scope");
        b.attribute(AttrValue::Tracking(xarast_geom::Mp::new(30)))
            .expect("tracking");
        for (i, c) in "Ta\u{1}b".chars().enumerate() {
            b.node(NodeKind::TextItem(TextItem::Char(c))).expect("char");
            if i == 0 {
                b.node(NodeKind::TextItem(TextItem::Kern(xarast_geom::Mp::new(
                    -60,
                ))))
                .expect("kern");
            }
            if i == 1 {
                b.push_scope().expect("scope");
                b.attribute(AttrValue::Script(Script {
                    on: true,
                    offset: 0.33,
                    size: 0.6,
                }))
                .expect("script");
                b.pop_scope();
            }
        }
        b.node(NodeKind::TextItem(TextItem::LineBreak(false)))
            .expect("soft");
        b.pop_scope();
        b.node(NodeKind::TextLine(Box::default())).expect("line");
        b.node(NodeKind::TextLine(Box::default())).expect("line");
        b.push_scope().expect("scope");
        b.attribute(AttrValue::LineSpace(LineSpacing::Ratio(1.5)))
            .expect("spacing");
        b.node(NodeKind::TextItem(TextItem::Char('z')))
            .expect("char");
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .expect("eol");
        b.pop_scope();
        b.pop_scope();
    }
    let (doc, _) = b.finish().expect("document");
    for (name, opts) in [
        ("writer.svg", SvgOptions::default()),
        (
            "writer-plain.svg",
            SvgOptions {
                hoist: false,
                classes: false,
                ..SvgOptions::default()
            },
        ),
    ] {
        let mut res = ResourceIndex::new();
        std::fs::write(dir.join(name), write_svg(&doc, &mut res, &opts).svg)?;
    }
    std::fs::write(
        dir.join("third-party.svg"),
        r##"<?xml version="1.0"?>
<!-- Created by hand -->
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:acme="urn:acme" viewBox="0 0 200 100">
<style>.a{fill:#123;stroke:red} .b{stroke-width:2}</style>
<defs><linearGradient id="g0"><stop offset="0" stop-color="gold"/><stop offset="100%" style="stop-color:rgb(0,0,255);stop-opacity:.5"/></linearGradient>
<linearGradient id="g1" xlink:href="#g0" x1="0" y1="0" x2="1" y2="1" gradientTransform="rotate(30)"/></defs>
<g transform="translate(10 10) scale(2)" acme:layer="1"><rect class="a b" x="1" y="2" width="30" height="20" rx="3"/>
<path d="M0 50 q 20 -20 40 0 t 40 0 a 10 5 30 1 0 20 0 z m5 5 h10 v10 H5 Z" fill="url(#g1) red" acme:x="y"><acme:note>keep</acme:note></path>
<circle cx="80" cy="30" r="10" style="fill:none;stroke:#00f;stroke-dasharray:2 1"/><polyline points="1,1 5,9 9,1"/>
<text x="10" y="90" font-family="Arial" font-size="12">Hi <tspan font-weight="bold">there</tspan></text><?acme pi?></g>
<image x="100" y="0" width="20" height="20" href="data:image/png;base64,iVBORw0KGgo="/>
</svg>
"##,
    )?;
    Ok(())
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
    svg_seeds(Path::new(&root))?;
    std::fs::write(
        manifest.join("digests.xml"),
        r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"><mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip"/><mf:file-entry mf:full-path="resources/images/b3-0123456789abcdef0123456789abcdef.png" mf:media-type="image/png" mf:role="resource" mf:size="10" mf:method="stored" mf:digest="blake3-256" mf:digest-value="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" mf:refcount="3" mf:derived-from="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" mf:derivation="crop 1 2 3 4"/></mf:manifest>"#,
    )?;
    Ok(())
}
