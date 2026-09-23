//! Container budgets of `docs/phases/phase-06-xarast-format.md`.
//!
//! | Bench | Budget |
//! |---|---|
//! | `save_20mb` | save a 20 MB `.xarast` ≤ 1 s (container part: no SVG serialisation yet) |
//! | `save_20mb_atomic` | the same through `write_atomic` to disk, fsyncs included |
//! | `resave_300mb_raw` | re-save a 300 MB photo document with unchanged resources ≤ 1 s |
//! | `open_20mb` | signature + manifest + thumbnail of a 20 MB file ≤ 15 ms |
//! | `blake3_64mb` | ≥ 1 GB/s per core |
//!
//! The 20 MB document is 12 MiB of SVG-like path text (compressible, as a
//! real `document.svg` is) plus 8 MiB of incompressible "JPEG" resources in
//! eight files, which is the mix the budget is about. Everything is generated
//! here; no corpus is needed.
//!
//! ```text
//! cargo bench -p xarast-format --bench container
//! ```

use std::hint::black_box;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use xarast_format::digest::Digest;
use xarast_format::{PackageWriter, ResourceIndex, ResourceKind, WriteOptions, XarastReader};

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// SVG-like text: paths with pseudo-random coordinates, about as
/// compressible as a real `document.svg` (5–8:1).
fn svg_text(len: usize) -> Vec<u8> {
    let mut s = String::with_capacity(len + 256);
    s.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\">\n",
    );
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut i = 0u64;
    while s.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let a = x % 60_000;
        let b = (x >> 20) % 60_000;
        let c = (x >> 40) % 997;
        s.push_str(&format!(
            "<path id=\"x{i}\" d=\"m{}.{} {}.{}c{c} -{} {} {c} {}.5 -{}z\" fill=\"#{:06x}\"/>\n",
            a / 100,
            a % 100,
            b / 100,
            b % 100,
            c / 3,
            c / 7,
            c * 2,
            c / 5,
            x & 0xff_ffff
        ));
        i += 1;
    }
    s.push_str("</svg>\n");
    s.into_bytes()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    v.extend(noise(20_000, 7));
    v
}

struct Doc {
    svg: Arc<[u8]>,
    meta: Arc<[u8]>,
    thumb: Arc<[u8]>,
    index: ResourceIndex,
}

fn doc(svg_len: usize, resources: usize, resource_len: usize) -> Doc {
    let mut index = ResourceIndex::new();
    for i in 0..resources {
        index
            .insert(
                ResourceKind::Image,
                "jpg",
                noise(resource_len, i as u64 + 11),
            )
            .unwrap();
    }
    Doc {
        svg: svg_text(svg_len).into(),
        meta: b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<meta/>\n"
            .to_vec()
            .into(),
        thumb: png(256, 180).into(),
        index,
    }
}

fn writer(d: &Doc) -> PackageWriter {
    let mut w = PackageWriter::new(WriteOptions::default());
    w.set_meta(d.meta.clone());
    w.set_document(d.svg.clone());
    w.set_thumbnail(d.thumb.clone()).unwrap();
    w.add_resources(&d.index);
    w
}

fn save(d: &Doc) -> Vec<u8> {
    let mut out = Cursor::new(Vec::with_capacity(24 << 20));
    writer(d).finish(&mut out).unwrap();
    out.into_inner()
}

fn benches(c: &mut Criterion) {
    let d20 = doc(12 << 20, 8, 1 << 20);
    let bytes20 = save(&d20);
    eprintln!(
        "20 MB document: {} bytes of input, {} bytes of package",
        d20.svg.len() + 8 * (1 << 20),
        bytes20.len()
    );

    let mut g = c.benchmark_group("container");
    g.sample_size(10).measurement_time(Duration::from_secs(10));
    g.throughput(Throughput::Bytes((d20.svg.len() + (8 << 20)) as u64));
    g.bench_function("save_20mb", |b| b.iter(|| black_box(save(&d20))));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench.xarast");
    g.bench_function("save_20mb_atomic", |b| {
        b.iter(|| xarast_format::write_atomic(&path, |f| writer(&d20).finish(f)).unwrap())
    });

    g.throughput(Throughput::Bytes(bytes20.len() as u64));
    g.bench_function("open_20mb", |b| {
        b.iter(|| {
            let mut r = XarastReader::open(Cursor::new(&bytes20[..])).unwrap();
            black_box(r.thumbnail().unwrap());
        })
    });

    // 300 MB of photographs, already in the package: re-saving must copy
    // them compressed, never rehash or recompress them.
    let d300 = doc(1 << 20, 30, 10 << 20);
    let bytes300 = save(&d300);
    g.throughput(Throughput::Bytes(bytes300.len() as u64));
    g.bench_function("resave_300mb_raw", |b| {
        b.iter(|| {
            let mut r = XarastReader::open(Cursor::new(&bytes300[..])).unwrap();
            let ix = ResourceIndex::from_package(&r);
            let mut w = PackageWriter::new(WriteOptions::default());
            w.set_meta(d300.meta.clone());
            w.set_document(d300.svg.clone());
            w.add_resources(&ix);
            w.carry_from(&r);
            let mut out = Cursor::new(Vec::with_capacity(bytes300.len() + (1 << 20)));
            w.finish_with_source(&mut out, Some(&mut r)).unwrap();
            black_box(out.into_inner().len())
        })
    });
    g.finish();

    let mut g = c.benchmark_group("digest");
    let data = noise(64 << 20, 3);
    g.throughput(Throughput::Bytes(data.len() as u64));
    g.sample_size(20);
    g.bench_function("blake3_64mb", |b| b.iter(|| black_box(Digest::of(&data))));
    g.finish();
}

criterion_group!(container, benches);
criterion_main!(container);
