//! The bomb suite (phase 10 acceptance criterion 2). Every fixture is built
//! here, never read from disk; each must be refused with the right variant,
//! in under a second, with peak RSS growth under 64 MiB and no partial
//! result.
//!
//! The list only grows. `docs/memory/image.md` records what each targets.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageFormat as F};
use xarast_image::{DecodeError, DecodeLimits, SizeLimit, decode, decode_on_worker, xar};

/// Every test here takes this lock: the RSS measurements must not see
/// another test's allocations.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What a fixture must be refused as.
#[derive(Debug, Clone, Copy)]
enum Want {
    TooLarge(SizeLimit),
    Ratio,
    Work,
    Corrupt,
    AnyError,
}

fn matches(e: &DecodeError, want: Want) -> bool {
    match (e, want) {
        (DecodeError::TooLarge { limit, .. }, Want::TooLarge(l)) => *limit == l,
        (DecodeError::SuspiciousRatio { .. }, Want::Ratio) => true,
        (DecodeError::ExcessiveWork { .. }, Want::Work) => true,
        (DecodeError::Corrupt(_) | DecodeError::Unsupported(_), Want::Corrupt) => true,
        (_, Want::AnyError) => true,
        _ => false,
    }
}

/// A PNG declaring 65 535 × 65 535 with a 1-row IDAT.
fn png_huge() -> Vec<u8> {
    support::png_raw(65_535, 65_535, 8, 6, &support::zlib(&[0; 64]))
}

/// A PNG declaring 16 000 × 16 000 grey (within every size limit) whose
/// IDAT is a few kilobytes: 256 MB promised from ~5 KB, far above 20 000:1.
fn png_zlib_bomb() -> Vec<u8> {
    let rows = vec![0u8; 16_001 * 64];
    support::png_raw(16_000, 16_000, 8, 0, &support::zlib(&rows))
}

/// A baseline JPEG with 5 000 extra (empty) scans spliced in before EOI.
fn jpeg_many_scans() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(16, 16, image::Rgb([90, 90, 90]));
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut out, F::Jpeg)
        .expect("jpeg");
    let jpeg = out.into_inner();
    let eoi = jpeg.len() - 2;
    let mut v = jpeg[..eoi].to_vec();
    // SOS: length 8, one component (id 1), tables 0/0, Ss=0 Se=63 AhAl=0.
    let sos = [0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00];
    for _ in 0..5_000 {
        v.extend_from_slice(&sos);
        v.push(0x00);
    }
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

/// A GIF whose logical screen is 65 535 × 65 535.
fn gif_huge_screen() -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend_from_slice(&65_535u16.to_le_bytes());
    v.extend_from_slice(&65_535u16.to_le_bytes());
    v.extend_from_slice(&[0x80, 0, 0]); // global table of 2 entries
    v.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
    // Image descriptor covering the screen, then a tiny LZW stream.
    v.push(0x2C);
    v.extend_from_slice(&[0, 0, 0, 0]);
    v.extend_from_slice(&65_535u16.to_le_bytes());
    v.extend_from_slice(&65_535u16.to_le_bytes());
    v.push(0);
    v.extend_from_slice(&[2, 2, 0x4C, 0x01, 0, 0x3B]);
    v
}

/// A WebP whose RIFF and VP8L headers disagree and whose bitstream is junk.
fn webp_inconsistent() -> Vec<u8> {
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&0xFFFF_FFF0u32.to_le_bytes()); // RIFF size lies
    v.extend_from_slice(b"WEBPVP8L");
    v.extend_from_slice(&5u32.to_le_bytes());
    // Signature 0x2F, then 14-bit width-1 and height-1 all ones: 16384².
    v.extend_from_slice(&[0x2F, 0xFF, 0xFF, 0xFF, 0xFF]);
    v.extend_from_slice(&[0xAB; 32]);
    v
}

/// A WebP VP8X extended header declaring a 16 777 216² canvas.
fn webp_vp8x_huge() -> Vec<u8> {
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&30u32.to_le_bytes());
    v.extend_from_slice(b"WEBPVP8X");
    v.extend_from_slice(&10u32.to_le_bytes());
    v.extend_from_slice(&[0, 0, 0, 0]);
    v.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    v
}

/// A TIFF declaring 65 535 × 65 535 with one tiny strip.
fn tiff_huge() -> Vec<u8> {
    let entries: [(u16, u16, u32, u32); 8] = [
        (256, 4, 1, 65_535), // width
        (257, 4, 1, 65_535), // height
        (258, 3, 1, 8),      // bits per sample
        (259, 3, 1, 1),      // no compression
        (262, 3, 1, 1),      // black is zero
        (273, 4, 1, 200),    // strip offset
        (278, 4, 1, 65_535), // rows per strip
        (279, 4, 1, 16),     // strip byte count
    ];
    let mut v = b"II*\0".to_vec();
    v.extend_from_slice(&8u32.to_le_bytes());
    v.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, value) in entries {
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&ty.to_le_bytes());
        v.extend_from_slice(&count.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
    }
    v.extend_from_slice(&0u32.to_le_bytes());
    v.resize(216, 0);
    v
}

/// A BMP declaring 60 000 × 60 000 at 32 bpp with no pixel data.
fn bmp_huge() -> Vec<u8> {
    let mut dib = Vec::new();
    for x in [40u32, 60_000, 60_000] {
        dib.extend_from_slice(&x.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&[0; 24]);
    xar::dib_to_bmp(&dib).expect("header")
}

/// A PPM declaring 4 000 000 000 × 1 pixels.
fn pnm_huge() -> Vec<u8> {
    b"P6\n4000000000 1\n255\n\0\0\0".to_vec()
}

/// A BMPZIP whose inflated payload would be far beyond the output limit:
/// 64 MiB of zeros deflated to ~64 KiB, under a 16 MiB output limit.
fn bmpzip_bomb() -> Vec<u8> {
    support::zlib(&vec![0u8; 64 << 20])
}

#[test]
fn every_bomb_is_refused_quickly_and_cheaply() {
    let _serial = serial();
    let limits = DecodeLimits::default();
    let cases: Vec<(&str, Vec<u8>, Want)> = vec![
        ("png 65535²", png_huge(), Want::TooLarge(SizeLimit::Pixels)),
        ("png zlib bomb", png_zlib_bomb(), Want::Ratio),
        ("jpeg 5000 scans", jpeg_many_scans(), Want::Work),
        (
            "gif huge screen",
            gif_huge_screen(),
            Want::TooLarge(SizeLimit::Pixels),
        ),
        ("webp inconsistent", webp_inconsistent(), Want::AnyError),
        ("webp vp8x huge", webp_vp8x_huge(), Want::AnyError),
        (
            "tiff 65535²",
            tiff_huge(),
            Want::TooLarge(SizeLimit::Pixels),
        ),
        ("bmp 60000²", bmp_huge(), Want::TooLarge(SizeLimit::Pixels)),
        (
            "pnm 4e9 wide",
            pnm_huge(),
            Want::TooLarge(SizeLimit::Dimension),
        ),
        ("truncated png", png_huge()[..40].to_vec(), Want::Corrupt),
    ];
    let bmpzip = bmpzip_bomb();
    let baseline = support::rss_kib();
    let reset = support::reset_peak_rss();
    for (name, bytes, want) in &cases {
        let t = Instant::now();
        let r = decode(bytes, &limits);
        let dt = t.elapsed();
        let e = r.err().unwrap_or_else(|| panic!("{name}: decoded a bomb"));
        assert!(matches(&e, *want), "{name}: got {e:?}, wanted {want:?}");
        assert!(
            !e.is_overridable() || matches!(want, Want::TooLarge(_)),
            "{name}"
        );
        assert!(dt < Duration::from_secs(1), "{name}: took {dt:?}");
        println!("{name}: {e} in {dt:?}");
    }
    // BMPZIP through the `.xar` wrapper, with tight limits.
    let t = Instant::now();
    let small = DecodeLimits {
        max_decoded_bytes: 16 << 20,
        ..DecodeLimits::tight()
    };
    let e = xar::decode_xar_bitmap(69, &bmpzip, &[], &small).expect_err("bmpzip bomb");
    assert!(matches!(e, DecodeError::SuspiciousRatio { .. }), "{e:?}");
    assert!(t.elapsed() < Duration::from_secs(1));

    if let (true, Some(base), Some(peak)) = (reset, baseline, support::peak_rss_kib()) {
        let growth_mib = peak.saturating_sub(base) / 1024;
        println!("peak RSS growth over the suite: {growth_mib} MiB");
        assert!(growth_mib < 64, "peak RSS grew {growth_mib} MiB");
    }
}

#[test]
fn the_decoders_alone_stay_under_64_mib() {
    let limits = DecodeLimits::default();
    let fixtures = [
        png_huge(),
        png_zlib_bomb(),
        jpeg_many_scans(),
        gif_huge_screen(),
        webp_inconsistent(),
        webp_vp8x_huge(),
        tiff_huge(),
        bmp_huge(),
        pnm_huge(),
    ];
    let (Some(base), true) = (support::rss_kib(), support::reset_peak_rss()) else {
        println!("skipping: no /proc peak-RSS reset on this platform");
        return;
    };
    for f in &fixtures {
        assert!(decode(f, &limits).is_err());
    }
    let peak = support::peak_rss_kib().unwrap_or(base);
    let growth_mib = peak.saturating_sub(base) / 1024;
    assert!(growth_mib < 64, "peak RSS grew {growth_mib} MiB");
}

#[test]
fn too_large_is_the_only_overridable_error_and_raising_the_limit_works() {
    let _serial = serial();
    let png = support::png_raw(3000, 3000, 8, 0, &support::zlib(&vec![0u8; 3001 * 3000]));
    let tight = DecodeLimits {
        max_pixels: 1_000_000,
        ..DecodeLimits::default()
    };
    let e = decode(&png, &tight).expect_err("too large");
    assert!(e.is_overridable());
    assert!(matches!(
        e,
        DecodeError::TooLarge {
            declared: (3000, 3000),
            limit: SizeLimit::Pixels
        }
    ));
    let d = decode(&png, &DecodeLimits::default()).expect("import anyway");
    assert_eq!(d.data.width, 3000);
    assert!(!DecodeError::SuspiciousRatio { ratio: 1 }.is_overridable());
    assert!(!DecodeError::AllocationRefused { wanted: 1 }.is_overridable());
    assert!(!DecodeError::Timeout.is_overridable());
}

#[test]
fn a_zero_duration_times_out_rather_than_returning_pixels() {
    let _serial = serial();
    let png = support::encode(
        &DynamicImage::ImageRgba8(support::pattern(256, 256, true)),
        F::Png,
    );
    let limits = DecodeLimits {
        max_duration: Duration::ZERO,
        ..DecodeLimits::default()
    };
    assert!(matches!(decode(&png, &limits), Err(DecodeError::Timeout)));
}

#[test]
fn the_worker_enforces_a_hard_deadline() {
    let _serial = serial();
    // A large but legitimate PNG under a 1 ms ceiling.
    let png = support::png_raw(4000, 4000, 8, 2, &support::zlib(&vec![0u8; 4000 * 12_001]));
    let limits = DecodeLimits {
        max_duration: Duration::from_millis(1),
        ..DecodeLimits::default()
    };
    let t = Instant::now();
    let r = decode_on_worker(Arc::from(png), limits);
    assert!(matches!(r, Err(DecodeError::Timeout)), "{r:?}");
    assert!(t.elapsed() < Duration::from_millis(500));
    // And a small one completes.
    let small = support::encode(
        &DynamicImage::ImageRgba8(support::pattern(8, 8, false)),
        F::Png,
    );
    let d = decode_on_worker(Arc::from(small), DecodeLimits::default()).expect("small");
    assert_eq!(d.data.width, 8);
}

#[test]
fn the_ratio_warning_fires_between_the_two_thresholds() {
    let _serial = serial();
    // 5000 × 4000 grey zeros: 20 MB native from ~20 KB, about 1 000:1.
    let png = support::png_raw(5000, 4000, 8, 0, &support::zlib(&vec![0u8; 5001 * 4000]));
    let limits = DecodeLimits {
        warn_ratio: 100,
        ..DecodeLimits::default()
    };
    let d = decode(&png, &limits).expect("decode");
    assert!(
        d.warnings
            .iter()
            .any(|w| matches!(w, xarast_image::DecodeWarning::HighRatio { .. }))
    );
}
