//! T10.4.3: the resampling comparison harness, and the tests that keep its
//! verdict true.
//!
//! A fixed synthetic test set — a resolution chart (zone plate), a
//! "photograph" (multi-octave value noise in three channels), a
//! screenshot (flat blocks, 1-px lines and a 1-px checker) and a logo
//! (thin antialiased rings) — is resampled at four magnification and four
//! minification ratios by every candidate kernel:
//!
//! * **Magnification** is scored by a round trip: the image is reduced by
//!   an exact area average in linear light, enlarged back with the
//!   candidate, and compared with the original (PSNR over encoded RGB, so
//!   sharpness retention and ringing both cost). Ringing is also measured
//!   on its own: how far an unclamped sample leaves the range of the 2 × 2
//!   texels around it, × 255 in the space the filter averaged in, on the
//!   two hard-edged images. Each kernel runs in linear light and in
//!   encoded sRGB, which is how `MAGNIFY_SPACE` was chosen.
//! * **Minification** is scored against the exact area average: the
//!   product sampler (the tent widened by the ratio below 2×, trilinear
//!   over the pyramid from 2×), each kernel widened by the ratio on the
//!   base, and plain bilinear with no prefilter (the aliasing baseline).
//!   The reference is a box, which is what the pyramid computes, so at
//!   ÷2 (and nearly at ÷4, ÷8) the pyramid matches it by construction:
//!   those rows show the pyramid is *correct*, not that a box is the best
//!   possible filter. ÷1.5 is the honest comparison between candidates.
//! * **Cost** is nanoseconds per output pixel; meaningful only in release:
//!   `cargo test --release -p xarast-render --test resampling -- --nocapture`.
//!
//! The numbers and the choice are recorded in `docs/memory/render.md`,
//! "Resampling quality". The assertions below are the parts of that
//! verdict that must stay true.

use std::time::Instant;

use xarast_render::resample::{
    ALL_KERNELS, HQ_KERNEL, ImageSampler, Kernel, Level, MAGNIFY_SPACE, Space, encode,
    encode_premul, encode_premul_in, kernel_sample, kernel_sample_in, to_linear,
};
use xarast_render::{Filter, GradMapping, ImageRef, Point64, Repeat};

const SIZE: usize = 128;

#[derive(Clone)]
struct Img {
    w: usize,
    h: usize,
    /// Straight sRGB RGBA8, opaque.
    data: Vec<u8>,
}

impl Img {
    fn level(&self) -> Level<'_> {
        Level {
            width: self.w as u32,
            height: self.h as u32,
            data: &self.data,
        }
    }
}

fn from_fn(f: impl Fn(usize, usize) -> [u8; 3]) -> Img {
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let [r, g, b] = f(x, y);
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Img {
        w: SIZE,
        h: SIZE,
        data,
    }
}

fn unit_to_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn hash(x: i64, y: i64, seed: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn value_noise(x: f64, y: f64, seed: u64) -> f64 {
    let mut v = 0.0;
    let mut amp = 0.5;
    for (octave, cell) in [32.0, 16.0, 8.0, 4.0, 2.0, 1.0].into_iter().enumerate() {
        let (fx, fy) = (x / cell, y / cell);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let s = seed + octave as u64 * 7919;
        let (x0, y0) = (x0 as i64, y0 as i64);
        let a = hash(x0, y0, s) + (hash(x0 + 1, y0, s) - hash(x0, y0, s)) * tx;
        let b = hash(x0, y0 + 1, s) + (hash(x0 + 1, y0 + 1, s) - hash(x0, y0 + 1, s)) * tx;
        v += amp * (a + (b - a) * ty);
        amp *= 0.55;
    }
    v
}

fn test_set() -> Vec<(&'static str, Img)> {
    let chart = from_fn(|x, y| {
        let (dx, dy) = (x as f64 - 63.5, y as f64 - 63.5);
        // Half a cycle per pixel at radius 64: the chart reaches Nyquist
        // at its inscribed circle.
        let v = 0.5 + 0.5 * (std::f64::consts::PI * (dx * dx + dy * dy) / 128.0).cos();
        let c = unit_to_u8(v);
        [c, c, c]
    });
    let photo = from_fn(|x, y| {
        let (x, y) = (x as f64, y as f64);
        [
            unit_to_u8(value_noise(x, y, 1)),
            unit_to_u8(value_noise(x, y, 2)),
            unit_to_u8(value_noise(x, y, 3)),
        ]
    });
    let screenshot = from_fn(|x, y| {
        if (8..56).contains(&x) && (8..40).contains(&y) {
            [40, 90, 200]
        } else if (64..120).contains(&x) && (8..40).contains(&y) {
            if (x + y) % 2 == 0 {
                [0, 0, 0]
            } else {
                [255, 255, 255]
            }
        } else if y % 8 == 0 && y > 48 {
            [20, 20, 20]
        } else if x % 12 == 3 && y > 48 {
            [200, 30, 30]
        } else {
            [245, 245, 240]
        }
    });
    let logo = from_fn(|x, y| {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let mut cov: f64 = 0.0;
        for (cx, cy, r) in [(64.0, 64.0, 50.0), (64.0, 64.0, 30.3), (40.0, 90.0, 12.7)] {
            let d: f64 = ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt() - r;
            // A 1.5-px ring, antialiased by distance.
            cov = cov.max((1.0 - (d.abs() - 0.75)).clamp(0.0, 1.0));
        }
        // Coverage is linear light; ink on paper.
        let v = encode((1.0 - cov) as f32 * 0.98 + 0.01);
        [v, v, encode((1.0 - cov * 0.6) as f32)]
    });
    vec![
        ("chart", chart),
        ("photo", photo),
        ("screenshot", screenshot),
        ("logo", logo),
    ]
}

/// The exact area average of `src` onto `n × n` texels, in linear light.
fn area_reduce(src: &Img, n: usize) -> Img {
    let r = src.w as f64 / n as f64;
    let weights = |i: usize| -> Vec<(usize, f64)> {
        let (a, b) = (i as f64 * r, (i + 1) as f64 * r);
        let mut out = Vec::new();
        for k in a.floor() as usize..(b.ceil() as usize).min(src.w) {
            let w = (b.min(k as f64 + 1.0) - a.max(k as f64)).max(0.0);
            if w > 0.0 {
                out.push((k, w / r));
            }
        }
        out
    };
    let mut data = Vec::with_capacity(n * n * 4);
    for j in 0..n {
        let wy = weights(j);
        for i in 0..n {
            let wx = weights(i);
            let mut acc = [0.0f64; 3];
            for &(y, a) in &wy {
                for &(x, b) in &wx {
                    let o = (y * src.w + x) * 4;
                    for (c, slot) in acc.iter_mut().enumerate() {
                        *slot += f64::from(to_linear(src.data[o + c])) * a * b;
                    }
                }
            }
            data.extend_from_slice(&[
                encode(acc[0] as f32),
                encode(acc[1] as f32),
                encode(acc[2] as f32),
                255,
            ]);
        }
    }
    Img { w: n, h: n, data }
}

fn psnr(a: &Img, b: &Img) -> f64 {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let mut se = 0.0;
    let mut n = 0.0;
    for (pa, pb) in a
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data.as_chunks::<4>().0)
    {
        for c in 0..3 {
            let d = f64::from(pa[c]) - f64::from(pb[c]);
            se += d * d;
            n += 1.0;
        }
    }
    let mse = se / n;
    if mse == 0.0 {
        99.0
    } else {
        10.0 * (255.0 * 255.0 / mse).log10()
    }
}

struct Enlarged {
    img: Img,
    /// Mean and maximum overshoot, linear light × 255.
    ring_mean: f64,
    ring_max: f64,
    ns_per_px: f64,
}

/// Enlarges `src` to `SIZE × SIZE` with a kernel.
fn enlarge(src: &Img, kernel: Kernel, space: Space) -> Enlarged {
    let table = |c: u8| match space {
        Space::Linear => to_linear(c),
        Space::Encoded => f32::from(c) / 255.0,
    };
    let scale = src.w as f64 / SIZE as f64;
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    let (mut ring, mut ring_max, mut count) = (0.0f64, 0.0f64, 0.0f64);
    let start = Instant::now();
    for j in 0..SIZE {
        let y = (j as f64 + 0.5) * scale - 0.5;
        for i in 0..SIZE {
            let x = (i as f64 + 0.5) * scale - 0.5;
            let p = kernel_sample_in(src.level(), Repeat::Simple, kernel, x, y, 1.0, space);
            let (x0, y0) = (x.floor() as i64, y.floor() as i64);
            for c in 0..3 {
                let mut lo = f32::MAX;
                let mut hi = f32::MIN;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let t = src.level().texel(x0 + dx, y0 + dy, Repeat::Simple);
                    let v = table([t.r, t.g, t.b][c]);
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
                let o = f64::from((p[c] - hi).max(lo - p[c]).max(0.0)) * 255.0;
                ring += o;
                ring_max = ring_max.max(o);
                count += 1.0;
            }
            let c = encode_premul_in(p, space);
            data.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    let ns_per_px = start.elapsed().as_nanos() as f64 / (SIZE * SIZE) as f64;
    Enlarged {
        img: Img {
            w: SIZE,
            h: SIZE,
            data,
        },
        ring_mean: ring / count,
        ring_max,
        ns_per_px,
    }
}

/// Reduces `src` to `n × n` with a kernel widened by the ratio, or with no
/// prefilter at all (`stretch = 1`).
fn reduce_with_kernel(src: &Img, n: usize, kernel: Kernel, prefilter: bool) -> (Img, f64) {
    let r = src.w as f64 / n as f64;
    let mut data = Vec::with_capacity(n * n * 4);
    let start = Instant::now();
    for j in 0..n {
        for i in 0..n {
            let (x, y) = ((i as f64 + 0.5) * r - 0.5, (j as f64 + 0.5) * r - 0.5);
            let s = if prefilter { r } else { 1.0 };
            let c = encode_premul(kernel_sample(src.level(), Repeat::Simple, kernel, x, y, s));
            data.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / (n * n) as f64;
    (Img { w: n, h: n, data }, ns)
}

/// Reduces `src` through the product sampler (`HighQuality`).
fn reduce_with_product(src: &Img, n: usize) -> (Img, f64) {
    let image = ImageRef::new(src.w as u32, src.h as u32, src.data.clone());
    let side = n as f64;
    let mapping = GradMapping::Affine {
        a: Point64::new(0.0, 0.0),
        b: Point64::new(0.0, side),
        c: Point64::new(side, 0.0),
    };
    let sampler = ImageSampler::new(&image, mapping, Repeat::Simple, Filter::HighQuality, None)
        .expect("a valid mapping");
    // Build the pyramid outside the timing: it is paid once per image.
    let _ = image.level_count();
    let mut data = Vec::with_capacity(n * n * 4);
    let start = Instant::now();
    for j in 0..n {
        for i in 0..n {
            let c = sampler.sample(Point64::new(i as f64 + 0.5, j as f64 + 0.5));
            data.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / (n * n) as f64;
    (Img { w: n, h: n, data }, ns)
}

const MAGNIFY: [f64; 4] = [1.5, 2.0, 3.0, 4.0];
const MINIFY: [f64; 4] = [1.5, 2.0, 4.0, 8.0];

#[derive(Default)]
struct Score {
    psnr: f64,
    ring_mean: f64,
    ring_max: f64,
    ns: f64,
    n: f64,
}

#[test]
fn the_resampling_harness_supports_the_kernel_choice() {
    let set = test_set();

    // Magnification.
    let mut mag: Vec<(Space, Kernel, Score)> = Vec::new();
    let mut hard_ring: Vec<(Space, Kernel, f64)> = Vec::new();
    for space in [Space::Linear, Space::Encoded] {
        for k in ALL_KERNELS {
            mag.push((space, k, Score::default()));
            hard_ring.push((space, k, 0.0));
        }
    }
    println!(
        "\n# Magnification (round trip PSNR dB; ringing mean/max × 255 in the filter's space)"
    );
    for (name, img) in &set {
        for ratio in MAGNIFY {
            let n = (SIZE as f64 / ratio).round() as usize;
            let small = area_reduce(img, n);
            for space in [Space::Linear, Space::Encoded] {
                let mut line = format!("{name:>10} ×{ratio:<3} {space:?}");
                for (i, (sp, k, s)) in mag.iter_mut().enumerate() {
                    if *sp != space {
                        continue;
                    }
                    let e = enlarge(&small, *k, space);
                    let p = psnr(&e.img, img);
                    line += &format!(
                        "  {}: {p:5.2} / {:.2} / {:.1}",
                        k.name(),
                        e.ring_mean,
                        e.ring_max
                    );
                    s.psnr += p;
                    s.ring_mean += e.ring_mean;
                    s.ring_max = s.ring_max.max(e.ring_max);
                    s.ns += e.ns_per_px;
                    s.n += 1.0;
                    if *name == "screenshot" || *name == "logo" {
                        hard_ring[i].2 += e.ring_mean;
                    }
                }
                println!("{line}");
            }
        }
    }
    println!("\n| space | kernel | mean PSNR | mean ringing | max ringing | ns/px |");
    for (space, k, s) in &mag {
        println!(
            "| {space:?} | {} | {:.2} | {:.3} | {:.1} | {:.0} |",
            k.name(),
            s.psnr / s.n,
            s.ring_mean / s.n,
            s.ring_max,
            s.ns / s.n
        );
    }

    // Minification.
    println!("\n# Minification (PSNR dB against the exact area average)");
    let mut prod_total = 0.0;
    let mut naive_total = 0.0;
    let mut wide: Vec<(Kernel, f64)> = ALL_KERNELS.iter().map(|&k| (k, 0.0)).collect();
    let mut prod_ns = 0.0;
    let mut wide_ns: Vec<f64> = vec![0.0; ALL_KERNELS.len()];
    let mut count = 0.0;
    for (name, img) in &set {
        for ratio in MINIFY {
            let n = (SIZE as f64 / ratio).round() as usize;
            let reference = area_reduce(img, n);
            let (prod, ns) = reduce_with_product(img, n);
            let (naive, _) = reduce_with_kernel(img, n, Kernel::Triangle, false);
            let p_prod = psnr(&prod, &reference);
            let p_naive = psnr(&naive, &reference);
            prod_total += p_prod;
            naive_total += p_naive;
            prod_ns += ns;
            let mut line = format!(
                "{name:>10} ÷{ratio:<3}  product: {p_prod:5.2}  no prefilter: {p_naive:5.2}"
            );
            for (i, &k) in ALL_KERNELS.iter().enumerate() {
                let (w, ns) = reduce_with_kernel(img, n, k, true);
                let p = psnr(&w, &reference);
                wide[i].1 += p;
                wide_ns[i] += ns;
                line += &format!("  {} widened: {p:5.2}", k.name());
            }
            count += 1.0;
            println!("{line}");
        }
    }
    println!(
        "\nproduct (tent below 2×, trilinear above) {:.2} dB at {:.0} ns/px; no prefilter {:.2} dB",
        prod_total / count,
        prod_ns / count,
        naive_total / count
    );
    for (i, (k, p)) in wide.iter().enumerate() {
        println!(
            "{} widened by the ratio: {:.2} dB at {:.0} ns/px",
            k.name(),
            p / count,
            wide_ns[i] / count
        );
    }

    // The verdict's load-bearing parts.
    assert_eq!(HQ_KERNEL, Kernel::MitchellNetravali);
    let ring = |k: Kernel| {
        hard_ring
            .iter()
            .find(|(sp, x, _)| *sp == MAGNIFY_SPACE && *x == k)
            .map(|(_, _, r)| *r)
            .unwrap()
    };
    let quality_in = |space: Space, k: Kernel| {
        mag.iter()
            .find(|(sp, x, _)| *sp == space && *x == k)
            .map(|(_, _, s)| s.psnr / s.n)
            .unwrap()
    };
    let quality = |k: Kernel| quality_in(MAGNIFY_SPACE, k);
    // Least ringing of the cubic-or-better candidates on hard edges…
    assert!(ring(HQ_KERNEL) < ring(Kernel::CatmullRom));
    assert!(ring(HQ_KERNEL) < ring(Kernel::Lanczos3));
    // …while still reconstructing better than bilinear.
    assert!(quality(HQ_KERNEL) > quality(Kernel::Triangle));
    // Magnification averages in encoded sRGB: better than linear light
    // for the product's kernels, with less ringing.
    assert_eq!(MAGNIFY_SPACE, Space::Encoded);
    for k in [Kernel::Triangle, HQ_KERNEL] {
        assert!(quality_in(Space::Encoded, k) > quality_in(Space::Linear, k));
    }
    // The pyramid is what makes minification correct: far better than
    // sampling the base with no prefilter.
    assert!(prod_total / count > naive_total / count + 3.0);
}

#[test]
fn an_aligned_mapping_samples_every_filter_as_a_point() {
    let set = test_set();
    let img = &set[1].1;
    let image = ImageRef::new(img.w as u32, img.h as u32, img.data.clone());
    // One texel per pixel, the image's top-left texel at device (10, 20),
    // and a mirrored copy.
    let s = SIZE as f64;
    let straight = GradMapping::Affine {
        a: Point64::new(10.0, 20.0),
        b: Point64::new(10.0, 20.0 + s),
        c: Point64::new(10.0 + s, 20.0),
    };
    let mirrored = GradMapping::Affine {
        a: Point64::new(10.0 + s, 20.0),
        b: Point64::new(10.0 + s, 20.0 + s),
        c: Point64::new(10.0, 20.0),
    };
    for mapping in [straight, mirrored] {
        let nearest = ImageSampler::new(&image, mapping, Repeat::Repeat, Filter::Nearest, None)
            .expect("valid");
        for filter in [Filter::Bilinear, Filter::HighQuality] {
            let s =
                ImageSampler::new(&image, mapping, Repeat::Repeat, filter, None).expect("valid");
            assert!(s.is_aligned());
            for y in (0..200).step_by(3) {
                for x in (0..200).step_by(3) {
                    let p = Point64::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                    assert_eq!(s.sample(p), nearest.sample(p), "{filter:?} at ({x}, {y})");
                }
            }
        }
    }
    // The fast path agrees with point sampling through the mapping: the
    // slow path it replaces. A shifted-by-a-half mapping is not aligned.
    let half = GradMapping::Affine {
        a: Point64::new(10.5, 20.0),
        b: Point64::new(10.5, 20.0 + s),
        c: Point64::new(10.5 + s, 20.0),
    };
    let s = ImageSampler::new(&image, half, Repeat::Repeat, Filter::Bilinear, None).expect("valid");
    assert!(!s.is_aligned());
}

#[test]
fn the_aligned_fast_path_is_the_point_sampled_slow_path() {
    // Pin the fast path (integer texel arithmetic) to the generic point
    // sampler through the frame map, bit for bit, over every repeat mode
    // and both orientations.
    let set = test_set();
    let img = &set[2].1;
    let image = ImageRef::new(img.w as u32, img.h as u32, img.data.clone());
    let s = SIZE as f64;
    for repeat in xarast_render::ALL_REPEATS {
        for (a, c) in [(6.0, 6.0 + s), (6.0 + s, 6.0)] {
            let aligned = GradMapping::Affine {
                a: Point64::new(a, 5.0),
                b: Point64::new(a, 5.0 + s),
                c: Point64::new(c, 5.0),
            };
            let fast =
                ImageSampler::new(&image, aligned, repeat, Filter::Nearest, None).expect("valid");
            assert!(fast.is_aligned());
            let frame = aligned.frame_map().expect("valid");
            for y in -40..200 {
                for x in -40..300 {
                    let p = Point64::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                    let (u, v) = frame.apply(p).expect("affine");
                    let slow = image.texel(
                        (u * SIZE as f64 - 0.5).round() as i64,
                        (v * SIZE as f64 - 0.5).round() as i64,
                        repeat,
                    );
                    assert_eq!(fast.sample(p), slow, "{repeat:?} ({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn a_perspective_mapping_picks_its_level_per_pixel() {
    // A 1-px checker receding in perspective: the far edge is minified
    // many times and must average to the checker's mean light (188), not
    // alias to black or white.
    let n = 256u32;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let image = ImageRef::new(n, n, data);
    let mapping = GradMapping::Perspective {
        a: Point64::new(0.0, 100.0),
        b: Point64::new(47.0, 0.0),
        c: Point64::new(100.0, 100.0),
        d: Point64::new(53.0, 0.0),
    };
    let s = ImageSampler::new(&image, mapping, Repeat::Simple, Filter::HighQuality, None)
        .expect("valid");
    for x in 48..52 {
        let c = s.sample(Point64::new(f64::from(x) + 0.5, 2.5));
        assert!((170..=205).contains(&c.r), "far edge at x = {x}: {c:?}");
    }
    assert!(image.has_pyramid());
}
