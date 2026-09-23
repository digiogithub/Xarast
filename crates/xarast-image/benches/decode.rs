//! Phase 10 decode budgets (`docs/phases/phase-10-bitmaps-and-photo.md`,
//! "Performance budgets"): probe ≤ 200 µs, 24 Mpx baseline JPEG ≤ 400 ms,
//! 24 Mpx PNG ≤ 600 ms, BLAKE3 of 100 MB ≤ 200 ms. The 4K PNG is the
//! everyday case.
//!
//! Every input is synthesised here: a smooth photographic field plus
//! deterministic noise, so the encoders produce realistic, not degenerate,
//! file sizes.

use std::hint::black_box;
use std::io::Cursor;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use xarast_image::{BitmapData, DecodeLimits, decode, probe};

/// Photographic-ish RGB8: gradients, a few soft discs, ±12 of noise.
fn photo(w: u32, h: u32) -> Vec<u8> {
    let mut seed = 0x1234_5678u32;
    let mut out = Vec::with_capacity(w as usize * h as usize * 3);
    for y in 0..h {
        for x in 0..w {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let n = (seed % 25) as i32 - 12;
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let d = ((fx - 0.4).powi(2) + (fy - 0.5).powi(2)).sqrt();
            let base = [
                (fx * 200.0 + 30.0 * (fy * 9.0).sin()) as i32,
                (fy * 180.0 + 60.0 * (1.0 - d)) as i32,
                (120.0 + 100.0 * (fx * 7.0 + fy * 3.0).cos()) as i32,
            ];
            for c in base {
                out.push((c + n).clamp(0, 255) as u8);
            }
        }
    }
    out
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut v = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut v, 90)
        .write_image(&photo(w, h), w, h, ExtendedColorType::Rgb8)
        .expect("encode jpeg");
    v.into_inner()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    PngEncoder::new_with_quality(&mut v, CompressionType::Default, FilterType::Adaptive)
        .write_image(&photo(w, h), w, h, ExtendedColorType::Rgb8)
        .expect("encode png");
    v
}

fn benches(c: &mut Criterion) {
    let limits = DecodeLimits::default();
    let j24 = jpeg(6000, 4000);
    let p4k = png(3840, 2160);
    let p24 = png(6000, 4000);
    println!(
        "fixtures: 24 Mpx JPEG {:.1} MB, 4K PNG {:.1} MB, 24 Mpx PNG {:.1} MB",
        j24.len() as f64 / 1e6,
        p4k.len() as f64 / 1e6,
        p24.len() as f64 / 1e6
    );

    let mut g = c.benchmark_group("probe");
    g.bench_function("jpeg_24mpx", |b| b.iter(|| probe(black_box(&j24))));
    g.bench_function("png_24mpx", |b| b.iter(|| probe(black_box(&p24))));
    g.finish();

    let mut g = c.benchmark_group("decode");
    g.sample_size(10).measurement_time(Duration::from_secs(8));
    g.bench_function("jpeg_24mpx", |b| {
        b.iter(|| decode(black_box(&j24), &limits).expect("jpeg"));
    });
    g.bench_function("png_4k", |b| {
        b.iter(|| decode(black_box(&p4k), &limits).expect("png"));
    });
    g.bench_function("png_24mpx", |b| {
        b.iter(|| decode(black_box(&p24), &limits).expect("png"));
    });
    g.finish();

    let hundred_mb = BitmapData {
        width: 5000,
        height: 5000,
        pixels: vec![0x5Au8; 100_000_000].into_boxed_slice(),
        deep: None,
    };
    let mut g = c.benchmark_group("hash");
    g.sample_size(20);
    g.bench_function("blake3_100mb", |b| {
        b.iter(|| black_box(&hundred_mb).content_hash())
    });
    g.finish();
}

criterion_group!(decode_benches, benches);
criterion_main!(decode_benches);
