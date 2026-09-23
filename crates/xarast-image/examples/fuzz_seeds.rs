//! Writes the seed corpus for `fuzz_image_decode`: one small, valid image
//! per format and per interesting layout, generated here — never taken
//! from the `.xar` corpus or anywhere else.
//!
//! ```sh
//! cargo run -p xarast-image --example fuzz_seeds -- /tmp/image-seeds
//! ```
//!
//! Seeds land in `<dir>/fuzz_image_decode/`. The target's first byte picks
//! the entry point: `1` is the sniffing façade, `0` the `.xar` wrapper
//! (followed by a tag selector, a palette length and the palette).

use std::io::Cursor;
use std::path::PathBuf;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn pattern(w: u32, h: u32, alpha: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = if alpha {
            (x * 255 / w.max(1)) as u8
        } else {
            255
        };
        Rgba([
            (x * 37 + y * 11) as u8,
            (y * 53) as u8,
            ((x ^ y) * 29) as u8,
            a,
        ])
    })
}

fn encode(img: &DynamicImage, f: ImageFormat) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    img.write_to(&mut c, f).expect("encode seed");
    c.into_inner()
}

fn main() {
    let dir: PathBuf = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: fuzz_seeds <dir>")
        .join("fuzz_image_decode");
    std::fs::create_dir_all(&dir).expect("create seed dir");
    let rgba = DynamicImage::ImageRgba8(pattern(12, 7, true));
    let rgb = DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(pattern(12, 7, false)).to_rgb8());
    let grey = DynamicImage::ImageLuma8(rgb.to_luma8());
    let rgb16 = DynamicImage::ImageRgb16(rgb.to_rgb16());
    let seeds: Vec<(String, Vec<u8>)> = vec![
        ("png_rgba".into(), encode(&rgba, ImageFormat::Png)),
        ("png_grey".into(), encode(&grey, ImageFormat::Png)),
        ("png_rgb16".into(), encode(&rgb16, ImageFormat::Png)),
        ("jpeg".into(), encode(&rgb, ImageFormat::Jpeg)),
        ("webp_lossless".into(), encode(&rgba, ImageFormat::WebP)),
        ("gif".into(), encode(&rgba, ImageFormat::Gif)),
        ("tiff".into(), encode(&rgba, ImageFormat::Tiff)),
        ("bmp".into(), encode(&rgba, ImageFormat::Bmp)),
        ("pnm".into(), encode(&rgb, ImageFormat::Pnm)),
    ];
    let mut out: Vec<(String, Vec<u8>)> = seeds
        .iter()
        .map(|(n, b)| {
            let mut v = vec![1u8];
            v.extend_from_slice(b);
            (format!("facade_{n}"), v)
        })
        .collect();
    // The `.xar` wrapper: a headerless DIB (tag 65, selector 1), the same
    // DIB deflated (tag 69, selector 5), and a JPEG with a palette (tag 71,
    // selector 6).
    let bmp = encode(&rgb, ImageFormat::Bmp);
    let dib = bmp[14..].to_vec();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut z, &dib).expect("deflate");
    let zdib = z.finish().expect("deflate");
    let mut xar_dib = vec![0u8, 1, 0];
    xar_dib.extend(&dib);
    let mut xar_zip = vec![0u8, 5, 0];
    xar_zip.extend(&zdib);
    let mut xar_jpeg8 = vec![0u8, 6, 3, 200, 30, 30, 30, 30, 200, 240, 240, 240];
    xar_jpeg8.extend(encode(&rgb, ImageFormat::Jpeg));
    out.push(("xar_dib".into(), xar_dib));
    out.push(("xar_bmpzip".into(), xar_zip));
    out.push(("xar_jpeg8bpp".into(), xar_jpeg8));
    for (name, bytes) in out {
        std::fs::write(dir.join(name), bytes).expect("write seed");
    }
    println!("wrote seeds to {}", dir.display());
}
