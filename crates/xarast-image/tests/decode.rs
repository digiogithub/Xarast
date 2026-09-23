//! Per-format decode round trips, EXIF orientation, the `.xar` wrappings and
//! the resource model, all over synthetic fixtures.

mod support;

use std::sync::Arc;

use image::{DynamicImage, ImageFormat as F};
use support::{encode, pattern};
use xarast_image::{
    BitmapResource, ColourSpace, DecodeLimits, ImageFormat, Orientation, decode, decode_as,
    pixels::premultiply, probe, xar,
};

/// The premultiplied bytes a straight RGBA image should decode to.
fn premul(img: &image::RgbaImage) -> Vec<u8> {
    img.pixels()
        .flat_map(|p| {
            let [r, g, b, a] = p.0;
            [premultiply(r, a), premultiply(g, a), premultiply(b, a), a]
        })
        .collect()
}

fn lossless_round_trip(format: F, want: ImageFormat, src: &image::RgbaImage, bytes: &[u8]) {
    let limits = DecodeLimits::default();
    let d = decode(bytes, &limits).unwrap_or_else(|e| panic!("{format:?}: {e}"));
    assert_eq!(d.format, want);
    assert_eq!(
        (d.data.width, d.data.height),
        src.dimensions(),
        "{format:?}"
    );
    assert_eq!(
        &*d.data.pixels,
        &premul(src)[..],
        "{format:?} pixels differ"
    );
    assert_eq!(d.applied_orientation, Orientation::Normal);
    let p = probe(bytes).expect("probe");
    assert_eq!(p.format, want);
    assert_eq!((p.info.pixel_width, p.info.pixel_height), src.dimensions());
}

#[test]
fn png_rgba8_round_trips_exactly() {
    let src = pattern(37, 23, true);
    let bytes = encode(&DynamicImage::ImageRgba8(src.clone()), F::Png);
    lossless_round_trip(F::Png, ImageFormat::Png, &src, &bytes);
    let d = decode(&bytes, &DecodeLimits::default()).expect("decode");
    assert!(d.info.has_alpha);
    assert_eq!(d.info.depth, 32);
}

#[test]
fn png_sixteen_bit_and_grey_decode() {
    let src = pattern(16, 9, false);
    let rgb16 = DynamicImage::ImageRgba8(src.clone()).to_rgb16();
    let bytes = encode(&DynamicImage::ImageRgb16(rgb16), F::Png);
    lossless_round_trip(F::Png, ImageFormat::Png, &src, &bytes);

    let grey = DynamicImage::ImageRgba8(src).to_luma8();
    let bytes = encode(&DynamicImage::ImageLuma8(grey.clone()), F::Png);
    let d = decode(&bytes, &DecodeLimits::default()).expect("grey");
    let want: Vec<u8> = grey
        .pixels()
        .flat_map(|p| [p.0[0], p.0[0], p.0[0], 255])
        .collect();
    assert_eq!(&*d.data.pixels, &want[..]);
    assert!(!d.info.has_alpha);
}

#[test]
fn lossless_formats_round_trip_exactly() {
    let opaque = pattern(29, 17, false);
    let with_alpha = pattern(29, 17, true);
    for (format, want, src) in [
        (F::Bmp, ImageFormat::Bmp, &with_alpha),
        (F::Tiff, ImageFormat::Tiff, &with_alpha),
        (F::WebP, ImageFormat::WebP, &with_alpha),
        (F::Pnm, ImageFormat::Pnm, &opaque),
    ] {
        let img = if format == F::Pnm {
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(src.clone()).to_rgb8())
        } else {
            DynamicImage::ImageRgba8(src.clone())
        };
        let bytes = encode(&img, format);
        lossless_round_trip(format, want, src, &bytes);
    }
}

#[test]
fn gif_decodes_the_first_frame() {
    // Few colours, so the quantiser keeps them exactly.
    let src = image::RgbaImage::from_fn(20, 10, |x, y| {
        if (x + y) % 2 == 0 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    });
    let bytes = encode(&DynamicImage::ImageRgba8(src.clone()), F::Gif);
    lossless_round_trip(F::Gif, ImageFormat::Gif, &src, &bytes);
}

#[test]
fn jpeg_decodes_close_to_the_source() {
    let src = image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 5) as u8, 128, 255])
    });
    let bytes = encode(
        &DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(src.clone()).to_rgb8()),
        F::Jpeg,
    );
    let d = decode(&bytes, &DecodeLimits::default()).expect("jpeg");
    assert_eq!(d.format, ImageFormat::Jpeg);
    assert_eq!((d.data.width, d.data.height), (64, 48));
    let max_err = d
        .data
        .pixels
        .iter()
        .zip(src.as_raw())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(max_err <= 24, "JPEG error {max_err}");
    assert!(!d.info.has_alpha);
    assert_eq!(d.info.depth, 24);
}

#[test]
fn all_eight_exif_orientations_decode_to_the_same_upright_pixels() {
    let upright = pattern(7, 4, true);
    let mut hashes = Vec::new();
    for o in 1..=8u16 {
        let stored = support::store_for_orientation(&upright, o);
        let png = encode(&DynamicImage::ImageRgba8(stored), F::Png);
        let bytes = support::png_with_exif(&png, o);
        let d = decode(&bytes, &DecodeLimits::default()).expect("decode");
        assert_eq!(d.applied_orientation.to_exif() as u16, o);
        assert_eq!((d.data.width, d.data.height), (7, 4), "orientation {o}");
        assert_eq!(&*d.data.pixels, &premul(&upright)[..], "orientation {o}");
        assert!(d.exif.is_some());
        let p = probe(&bytes).expect("probe");
        assert_eq!((p.info.pixel_width, p.info.pixel_height), (7, 4));
        hashes.push(d.data.content_hash());
    }
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn jpeg_exif_orientation_is_applied() {
    let src = image::RgbImage::from_fn(32, 16, |x, _| image::Rgb([(x * 8) as u8, 0, 0]));
    let jpeg = encode(&DynamicImage::ImageRgb8(src), F::Jpeg);
    let bytes = support::jpeg_with_exif(&jpeg, 6);
    let d = decode(&bytes, &DecodeLimits::default()).expect("jpeg");
    assert_eq!(d.applied_orientation, Orientation::Rot90);
    assert_eq!((d.data.width, d.data.height), (16, 32));
    // The JPEG probe fast path agrees with the decode.
    let p = probe(&bytes).expect("probe");
    assert_eq!(p.exif_orientation, Some(Orientation::Rot90));
    assert_eq!(p.info, d.info);
}

#[test]
fn a_headerless_dib_decodes_through_the_xar_wrapper() {
    let src = pattern(13, 5, false);
    let bmp = encode(
        &DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(src.clone()).to_rgb8()),
        F::Bmp,
    );
    let dib = &bmp[14..];
    let d = xar::decode_xar_bitmap(
        xar::TAG_DEFINEBITMAP_BMP,
        dib,
        &[],
        &DecodeLimits::default(),
    )
    .expect("dib");
    assert_eq!(d.format, ImageFormat::Dib);
    assert_eq!(&*d.data.pixels, &premul(&src)[..]);
    // The same bytes, deflated, under BMPZIP.
    let z = support::zlib(dib);
    let d2 = xar::decode_xar_bitmap(
        xar::TAG_DEFINEBITMAP_BMPZIP,
        &z,
        &[],
        &DecodeLimits::default(),
    )
    .expect("bmpzip");
    assert_eq!(d2.data, d.data);
    // A full BM file under tag 65 is sniffed and still decodes.
    let d3 = xar::decode_xar_bitmap(
        xar::TAG_DEFINEBITMAP_BMP,
        &bmp,
        &[],
        &DecodeLimits::default(),
    )
    .expect("bm under 65");
    assert_eq!(d3.data, d.data);
}

#[test]
fn an_indexed_dib_decodes() {
    let src = image::RgbImage::from_fn(9, 3, |x, _| {
        if x % 3 == 0 {
            image::Rgb([10, 20, 30])
        } else {
            image::Rgb([200, 100, 0])
        }
    });
    // Hand-built 8 bpp DIB: 40-byte header, 2 palette entries, rows padded
    // to 4 bytes, bottom-up.
    let (w, h) = (9u32, 3u32);
    let stride = (w as usize).div_ceil(4) * 4;
    let mut dib = Vec::new();
    for v in [40u32, w, h] {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&8u16.to_le_bytes());
    for v in [0u32, 0, 2835, 2835, 2, 0] {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    dib.extend_from_slice(&[30, 20, 10, 0, 0, 100, 200, 0]); // BGRX
    for y in (0..h).rev() {
        let mut row = vec![0u8; stride];
        for x in 0..w {
            row[x as usize] = u8::from(src.get_pixel(x, y).0 != [10, 20, 30]);
        }
        dib.extend(row);
    }
    let d = xar::decode_xar_bitmap(65, &dib, &[], &DecodeLimits::default()).expect("8bpp dib");
    let want: Vec<u8> = src
        .pixels()
        .flat_map(|p| [p.0[0], p.0[1], p.0[2], 255])
        .collect();
    assert_eq!(&*d.data.pixels, &want[..]);
    assert_eq!(d.info.depth, 8);
    assert_eq!(d.info.palette_entries, 2);
    assert_eq!((d.info.hdpi, d.info.vdpi), (72, 72));
}

#[test]
fn jpeg8bpp_snaps_back_onto_its_palette() {
    let palette = [[200u8, 30, 30], [30, 30, 200], [240, 240, 240]];
    let src = image::RgbImage::from_fn(32, 32, |x, y| {
        image::Rgb(palette[((x / 8 + y / 8) % 3) as usize])
    });
    let jpeg = encode(&DynamicImage::ImageRgb8(src.clone()), F::Jpeg);
    let d = xar::decode_xar_bitmap(
        xar::TAG_DEFINEBITMAP_JPEG8BPP,
        &jpeg,
        &palette,
        &DecodeLimits::default(),
    )
    .expect("jpeg8bpp");
    let want: Vec<u8> = src
        .pixels()
        .flat_map(|p| [p.0[0], p.0[1], p.0[2], 255])
        .collect();
    assert_eq!(
        &*d.data.pixels,
        &want[..],
        "every pixel back on the palette"
    );
    assert_eq!(d.info.depth, 8);
    assert_eq!(d.info.palette_entries, 3);
}

#[test]
fn png_colour_chunks_and_resolution_are_read() {
    let src = pattern(4, 4, false);
    let png = encode(&DynamicImage::ImageRgba8(src), F::Png);
    let ihdr_end = 8 + 12 + 13;
    let mut phys = Vec::new();
    phys.extend_from_slice(&11_811u32.to_be_bytes()); // 300 dpi
    phys.extend_from_slice(&11_811u32.to_be_bytes());
    phys.push(1);
    let mut bytes = png[..ihdr_end].to_vec();
    bytes.extend(support::png_chunk(b"sRGB", &[0]));
    bytes.extend(support::png_chunk(b"pHYs", &phys));
    bytes.extend_from_slice(&png[ihdr_end..]);
    let d = decode(&bytes, &DecodeLimits::default()).expect("png");
    assert_eq!(d.info.colour_space, ColourSpace::Srgb);
    assert_eq!((d.info.hdpi, d.info.vdpi), (300, 300));
    // 4 px at 300 dpi = 4/300 in = 960 mp.
    assert_eq!(d.info.recommended_width, 960);
    assert!(d.warnings.is_empty());
}

#[test]
fn an_unannotated_image_is_assumed_srgb_at_96_dpi() {
    let bytes = encode(&DynamicImage::ImageRgba8(pattern(96, 2, false)), F::Png);
    let d = decode(&bytes, &DecodeLimits::default()).expect("png");
    assert_eq!(d.info.colour_space, ColourSpace::AssumedSrgb);
    assert_eq!(d.info.hdpi, 96);
    assert_eq!(d.info.recommended_width, 72_000);
}

#[test]
fn the_content_hash_ignores_the_container() {
    let src = pattern(31, 19, true);
    let png = encode(&DynamicImage::ImageRgba8(src.clone()), F::Png);
    let tiff = encode(&DynamicImage::ImageRgba8(src), F::Tiff);
    let a = decode(&png, &DecodeLimits::default()).expect("png");
    let b = decode(&tiff, &DecodeLimits::default()).expect("tiff");
    assert_eq!(a.data.content_hash(), b.data.content_hash());
    let ra = BitmapResource::from_decoded(Arc::from("a"), a, None);
    assert_eq!(ra.content_hash, b.data.content_hash());
}

#[test]
fn straight_output_matches_the_source_for_opaque_and_round_trips_for_translucent() {
    let src = pattern(40, 3, true);
    let png = encode(&DynamicImage::ImageRgba8(src.clone()), F::Png);
    let d = decode(&png, &DecodeLimits::default()).expect("png");
    let straight = d.data.to_straight_rgba8();
    for (s, p) in straight
        .as_chunks::<4>()
        .0
        .iter()
        .zip(d.data.pixels.as_chunks::<4>().0)
    {
        for i in 0..3 {
            assert_eq!(premultiply(s[i], s[3]), p[i]);
        }
    }
    let opaque = pattern(40, 3, false);
    let png = encode(&DynamicImage::ImageRgba8(opaque.clone()), F::Png);
    let d = decode(&png, &DecodeLimits::default()).expect("png");
    assert_eq!(d.data.to_straight_rgba8(), opaque.into_raw());
}

#[test]
fn decode_as_refuses_a_mismatched_format_cleanly() {
    let png = encode(&DynamicImage::ImageRgba8(pattern(4, 4, false)), F::Png);
    assert!(decode_as(&png, ImageFormat::Jpeg, &DecodeLimits::default()).is_err());
    assert!(decode(b"not an image", &DecodeLimits::default()).is_err());
    assert!(decode(&[], &DecodeLimits::default()).is_err());
}

/// Fuzz finding (`fuzz_image_decode`, 2026-09-23): a translucent WebP
/// stored under tag 71 had its colour channels overwritten with opaque
/// palette colours, leaving channels above alpha. Tag 71 only reconstructs
/// an opaque JPEG, as the original does; anything else is kept as decoded.
#[test]
fn jpeg8bpp_leaves_a_translucent_non_jpeg_alone() {
    let src = pattern(9, 5, true);
    let png = encode(&DynamicImage::ImageRgba8(src.clone()), F::Png);
    let d = xar::decode_xar_bitmap(71, &png, &[[255, 0, 0]], &DecodeLimits::default())
        .expect("png under 71");
    assert_eq!(&*d.data.pixels, &premul(&src)[..]);
    assert_eq!(d.info.palette_entries, 0);
    let mut px = premul(&src);
    xar::snap_to_palette(&mut px, &[[255, 0, 0], [0, 0, 255]]);
    for p in px.as_chunks::<4>().0 {
        assert!(p[..3].iter().all(|c| *c <= p[3]));
    }
}
