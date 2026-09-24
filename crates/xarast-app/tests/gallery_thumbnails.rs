//! The bitmap gallery's thumbnails through the shared decode path
//! (XARA-T-0293) are the ones the gallery made with its own decoder
//! before: the same pixels, byte for byte, whether the bitmap is decoded
//! for the thumbnail or found already decoded by the view.
//!
//! The reference below is the dispatch the gallery used to carry (the
//! façade, or tag 71 with the palette, 65, 69), kept here as the oracle.
//! The corpus is found through `XARAST_XAR_CORPUS` (default
//! `/home/user/xara-xtreme`) and never copied into this repository; with
//! no corpus that half skips with a notice.

use std::path::PathBuf;
use std::sync::Arc;

use xarast_app::bitmap_gallery::{Thumb, shrink, thumbnail};
use xarast_app::{DecodedImages, DocumentId, Session};
use xarast_doc::resources::{BitmapData, BitmapInfo, BitmapResource, ImageFormat, OriginalEncoded};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

/// The gallery's thumbnail as it was made before XARA-T-0293.
fn reference(res: &BitmapResource) -> Option<Thumb> {
    let (w, h) = (res.info.width, res.info.height);
    let expected = w as usize * h as usize * 4;
    if expected != 0 && res.pixels.pixels.len() == expected {
        return Some(shrink(w, h, &res.pixels.pixels));
    }
    use xarast_image::xar::decode_xar_bitmap;
    let o = res.original.as_ref()?;
    let bytes: &[u8] = &o.bytes;
    let palette: Vec<[u8; 3]> = res.pixels.palette.iter().map(|c| [c.r, c.g, c.b]).collect();
    let limits = xarast_image::DecodeLimits::default();
    let decoded = match o.format {
        ImageFormat::Jpeg if !palette.is_empty() => decode_xar_bitmap(71, bytes, &palette, &limits),
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif => {
            xarast_image::decode(bytes, &limits)
        }
        ImageFormat::Bmp => decode_xar_bitmap(65, bytes, &[], &limits),
        ImageFormat::Unknown => decode_xar_bitmap(69, bytes, &[], &limits),
    }
    .ok()?;
    let d = decoded.data;
    (d.width > 0 && d.height > 0).then(|| shrink(d.width, d.height, &d.to_straight_rgba8()))
}

fn gradient(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                77,
                ((x + y) % 256) as u8,
            ]
        })
        .collect()
}

fn resource(
    info: BitmapInfo,
    pixels: Vec<u8>,
    original: Option<OriginalEncoded>,
) -> BitmapResource {
    BitmapResource {
        name: Arc::from("t"),
        info,
        pixels: Arc::new(BitmapData {
            pixels: Arc::from(pixels),
            palette: Arc::from(Vec::new()),
        }),
        original: original.map(Arc::new),
        procedural: None,
        transparent_index: None,
    }
}

#[test]
fn synthetic_thumbnails_match_the_old_decoder() {
    let native = resource(
        BitmapInfo {
            width: 150,
            height: 70,
            bpp: 32,
            dpi_x: 96,
            dpi_y: 96,
        },
        gradient(150, 70),
        None,
    );
    let mut png = Vec::new();
    xarast_io::png::encode_png(
        &mut png,
        xarast_io::png::PngHeader {
            width: 130,
            height: 210,
            colour: xarast_io::PngColour::Rgba,
            depth: xarast_io::PngDepth::Eight,
            interlace: false,
            ppm: None,
            level: 1,
        },
        &gradient(130, 210),
    )
    .unwrap();
    let encoded = xarast_app::place::image_from_bytes(Arc::from(png), "g.png")
        .unwrap()
        .resource;
    let broken = resource(
        BitmapInfo::default(),
        Vec::new(),
        Some(OriginalEncoded {
            format: ImageFormat::Png,
            bytes: Arc::from(&b"\x89PNG\r\n\x1a\nnot really"[..]),
        }),
    );
    let neither = resource(BitmapInfo::default(), Vec::new(), None);
    for res in [&native, &encoded, &broken, &neither] {
        let images = DecodedImages::new();
        assert_eq!(thumbnail(res, &images), reference(res));
    }
    assert!(reference(&native).is_some() && reference(&encoded).is_some());
    assert!(reference(&broken).is_none() && reference(&neither).is_none());
}

fn corpus_files() -> Option<Vec<PathBuf>> {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if !root.is_dir() {
        assert!(
            std::env::var("XARAST_CORPUS_REQUIRED").as_deref() != Ok("1"),
            "XARAST_CORPUS_REQUIRED=1 but {} is not a directory",
            root.display()
        );
        eprintln!("skipping: no corpus at {}", root.display());
        return None;
    }
    Some(
        LOCK.lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let rel = l.split_whitespace().skip(2).collect::<Vec<_>>().join(" ");
                (!rel.is_empty()).then(|| root.join(rel))
            })
            .collect(),
    )
}

#[test]
fn corpus_thumbnails_match_the_old_decoder() {
    let Some(files) = corpus_files() else { return };
    let mut bitmaps = 0;
    for path in &files {
        let session = Session::open(DocumentId(1), path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (id, res) in session.doc.resources.bitmaps() {
            let want = reference(res);
            assert!(want.is_some(), "{}: {id:?} decodes", path.display());
            // Decoded for the thumbnail, and found decoded by the view.
            let fresh = thumbnail(res, &DecodedImages::new());
            let shared = thumbnail(res, session.decoded_images());
            assert_eq!(fresh, want, "{}: {id:?}", path.display());
            assert_eq!(shared, want, "{}: {id:?}", path.display());
            bitmaps += 1;
        }
    }
    eprintln!("{bitmaps} corpus bitmaps thumbnailed identically");
    assert!(bitmaps >= 38, "{bitmaps}");
}
