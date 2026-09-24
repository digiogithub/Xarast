//! `<xarast:photo-ops>` (phase 10 W10.6, `research/06 §6.9`): a bitmap
//! object's photo operations survive a save and a reload exactly, and an
//! operation of a kind this version does not know survives an edit and a
//! save character for character (phase 10 acceptance criterion 14, the
//! `research/06 §8.7` conformance test applied to photo operations).

use std::io::Cursor;
use std::sync::Arc;

use xarast_doc::photo::{Levels, LevelsChannel, PhotoOp, PhotoOps, PhotoOrient, PixelRect};
use xarast_doc::{
    BitmapData, BitmapInfo, BitmapNode, BitmapResource, BuildLimits, CommandBus, Document,
    ImageFormat, NodeKind, OriginalEncoded, SetPhotoOps,
};
use xarast_format::svg::{SvgOptions, normal_form};
use xarast_format::{
    OpenOptions, PackageWriter, ResourceIndex, SaveOptions, WriteOptions, XarastReader,
    open_reader, save_opened_to, save_to,
};
use xarast_geom::{Point, Vector};

fn deterministic() -> SaveOptions {
    SaveOptions {
        write: WriteOptions::deterministic(),
        ..SaveOptions::default()
    }
}

fn package(doc: &Document) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let opts = SaveOptions {
        svg: SvgOptions::default(),
        ..deterministic()
    };
    save_to(doc, &mut out, &opts).expect("save");
    out.into_inner()
}

fn open(bytes: &[u8]) -> xarast_format::OpenedDocument<Cursor<Vec<u8>>> {
    open_reader(Cursor::new(bytes.to_vec()), &OpenOptions::default()).expect("open")
}

fn resave(o: &mut xarast_format::OpenedDocument<Cursor<Vec<u8>>>) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    save_opened_to(&o.document, &mut o.package, &mut out, &deterministic()).expect("re-save");
    out.into_inner()
}

fn svg_of(bytes: &[u8]) -> String {
    let mut r = XarastReader::open(Cursor::new(bytes.to_vec())).unwrap();
    String::from_utf8(r.document_bytes().unwrap()).unwrap()
}

/// The package with its `document.svg` replaced by `svg`.
fn repack(original: &[u8], svg: &str) -> Vec<u8> {
    let mut r = XarastReader::open(Cursor::new(original.to_vec())).unwrap();
    let mut res = ResourceIndex::from_package(&r);
    let meta = r.meta_bytes().unwrap();
    let mut w = PackageWriter::new(WriteOptions::deterministic());
    w.set_document(svg.as_bytes().to_vec());
    w.set_meta(meta);
    res.begin_recount();
    for rec in res.records().map(|r| r.path()).collect::<Vec<_>>() {
        res.count_path(&rec);
    }
    w.add_resources(&res);
    w.carry_from(&r);
    let mut out = Cursor::new(Vec::new());
    w.finish_with_source(&mut out, Some(&mut r)).unwrap();
    out.into_inner()
}

fn every_op() -> PhotoOps {
    PhotoOps {
        ops: vec![
            PhotoOp::Saturation(-0.35),
            PhotoOp::Crop(PixelRect {
                x: 3,
                y: 1,
                width: 20,
                height: 11,
            }),
            PhotoOp::Levels(Levels {
                channel: LevelsChannel::Green,
                in_lo: 12,
                in_hi: 230,
                out_lo: 4,
                out_hi: 251,
            }),
            PhotoOp::Levels(Levels {
                channel: LevelsChannel::All,
                in_lo: 0,
                in_hi: 200,
                out_lo: 0,
                out_hi: 255,
            }),
            PhotoOp::Gamma(1.0 / 3.0),
            PhotoOp::Brightness(0.1),
            PhotoOp::Contrast(-0.2),
            PhotoOp::Orient(PhotoOrient {
                turns: 3,
                flip: true,
            }),
            PhotoOp::Greyscale,
        ],
    }
    .normalised()
}

/// A document with one bitmap object carrying `ops`, and a triangle.
fn document(ops: PhotoOps) -> Document {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    let png: Arc<[u8]> = Arc::from(&b"\x89PNG\r\n\x1a\nnot really a png"[..]);
    let image = b.define_bitmap(BitmapResource {
        name: Arc::from("photo"),
        info: BitmapInfo::default(),
        pixels: Arc::new(BitmapData::default()),
        original: Some(Arc::new(OriginalEncoded {
            format: ImageFormat::Png,
            bytes: png,
        })),
        procedural: None,
        transparent_index: None,
    });
    b.node(NodeKind::Bitmap(Box::new(BitmapNode {
        image,
        origin: Point::raw(50_000, 500_000),
        major: Vector::raw(100_000, 0),
        minor: Vector::raw(0, -80_000),
        photo_ops: ops,
    })))
    .unwrap();
    b.finish().unwrap().0
}

fn the_bitmap(doc: &Document) -> (xarast_doc::NodeId, BitmapNode) {
    doc.tree
        .preorder(doc.tree.root())
        .find_map(|n| match doc.tree.kind(n) {
            Some(NodeKind::Bitmap(b)) => Some((n, (**b).clone())),
            _ => None,
        })
        .expect("a bitmap object")
}

#[test]
fn every_known_op_reads_back_to_the_same_bits_and_bytes() {
    let ops = every_op();
    assert_eq!(ops.ops.len(), 9);
    let doc = document(ops.clone());
    let first = package(&doc);
    let svg = svg_of(&first);
    assert!(
        svg.contains(
            "<xarast:photo-ops><xarast:op xarast:kind=\"crop\" xarast:rect=\"3 1 20 11\"/>"
        ),
        "{svg}"
    );
    assert!(
        svg.contains("xarast:kind=\"gamma\" xarast:value=\"0.33333334\""),
        "{svg}"
    );
    let mut o = open(&first);
    assert!(o.diagnostics.is_empty(), "{:?}", o.diagnostics);
    assert!(o.preservation.intact(), "{:?}", o.preservation);
    assert_eq!(the_bitmap(&o.document).1.photo_ops, ops);
    assert_eq!(normal_form(&o.document), normal_form(&doc));
    assert_eq!(resave(&mut o), first, "the first re-save is a fixed point");
    // Without operations nothing is written.
    let plain = svg_of(&package(&document(PhotoOps::new())));
    assert!(!plain.contains("photo-ops"), "{plain}");
}

#[test]
fn a_hand_written_chain_is_normalised_on_read() {
    let first = package(&document(PhotoOps::new()));
    let svg = svg_of(&first);
    let chain = "<xarast:photo-ops><xarast:op xarast:kind=\"contrast\" xarast:value=\"0.25\"/>\
                 <xarast:op xarast:kind=\"brightness\" xarast:value=\"0\"/>\
                 <xarast:op xarast:kind=\"brightness\" xarast:value=\"-0.5\"/></xarast:photo-ops>";
    let at = svg.find("<image ").expect("the image");
    let close = at + svg[at..].find("/>").expect("its end");
    let edited = format!("{}>{chain}</image>{}", &svg[..close], &svg[close + 2..]);
    let o = open(&repack(&first, &edited));
    assert!(o.diagnostics.is_empty(), "{:?}", o.diagnostics);
    assert_eq!(
        the_bitmap(&o.document).1.photo_ops.ops,
        vec![PhotoOp::Brightness(-0.5), PhotoOp::Contrast(0.25)]
    );
}

#[test]
fn an_unknown_op_survives_an_edit_and_a_save_character_for_character() {
    let first = package(&document(every_op()));
    let svg = svg_of(&first);
    // A `curves` op from a future version, with a child element and its
    // own spacing, between two known ops.
    let curves = "<xarast:op xarast:kind=\"curves\"  xarast:channel=\"rgb\" \
                  xarast:points=\"0 0, 64 40, 192 220, 255 255\"><xarast:point x=\"1\"/></xarast:op>";
    let at = svg
        .find("<xarast:op xarast:kind=\"gamma\"")
        .expect("the gamma op");
    let edited = format!("{}{curves}{}", &svg[..at], &svg[at..]);
    let pkg = repack(&first, &edited);
    let mut o = open(&pkg);
    assert!(o.diagnostics.is_empty(), "{:?}", o.diagnostics);
    let (node, bm) = the_bitmap(&o.document);
    assert!(
        !bm.photo_ops.is_editable(),
        "an unknown op makes the chain non-editable"
    );
    assert_eq!(bm.photo_ops.ops.len(), 10);
    assert!(
        matches!(&bm.photo_ops.ops[4], PhotoOp::Unknown { kind, raw }
        if &**kind == "curves" && &**raw == curves)
    );
    // Editing the chain is refused; the document is untouched.
    let mut bus = CommandBus::new();
    let refused = bus.dispatch(
        &mut o.document,
        &SetPhotoOps {
            node,
            ops: PhotoOps::new(),
            master: None,
            label: "Adjust Photo",
        },
    );
    assert!(refused.is_err());
    // An edit elsewhere: move the bitmap object itself.
    let mut tx = xarast_doc::Tx::begin(&mut o.document);
    tx.transform(
        node,
        xarast_geom::Matrix::translate(Vector::raw(1000, -2000)),
    )
    .unwrap();
    let t = tx.commit("Move");
    bus.history_mut().commit(&mut o.document, t);
    // A full save from the model, then a re-save of that.
    let saved = package(&o.document);
    let out = svg_of(&saved);
    assert!(out.contains(curves), "{out}");
    let mut again = open(&saved);
    assert_eq!(
        the_bitmap(&again.document).1.photo_ops,
        the_bitmap(&o.document).1.photo_ops
    );
    assert!(svg_of(&resave(&mut again)).contains(curves));
}
