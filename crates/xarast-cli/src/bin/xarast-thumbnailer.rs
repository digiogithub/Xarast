//! `xarast-thumbnailer`: the desktop's thumbnail for a `.xarast` file
//! (`research/06 §3.2.5`, phase 6 F7.3).
//!
//! ```text
//! xarast-thumbnailer [-s SIZE] INPUT OUTPUT.png
//! ```
//!
//! It is what `packaging/linux/xarast.thumbnailer` runs for file managers
//! (`Exec=xarast-thumbnailer -s %s %i %o`). It **extracts** the package's
//! own `thumbnail.png`, scaled down to at most SIZE pixels on the longer
//! side, and nothing else: it never parses `document.svg` and never
//! renders, because a file manager runs it on every file it lists, on
//! untrusted input. A package without a thumbnail, or with one that fails
//! the format's own checks, gets no thumbnail (exit status 1) and the file
//! manager falls back to its icon.

use std::path::PathBuf;
use std::process::ExitCode;

use xarast_format::XarastReader;
use xarast_format::thumbnail::{THUMBNAIL_MAX_PX, check_png};
use xarast_render::Surface;
use xarast_render::golden::{decode_png, encode_png};

const USAGE: &str = "usage: xarast-thumbnailer [-s SIZE] INPUT.xarast OUTPUT.png";

fn main() -> ExitCode {
    let mut size: u32 = 256;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "-s" || a == "--size" {
            match args.next().and_then(|v| v.to_str()?.parse().ok()) {
                Some(n) if n > 0 => size = n,
                _ => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            }
        } else if a == "-h" || a == "--help" {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let [input, output] = paths.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match thumbnail(input, size) {
        Ok(png) => match std::fs::write(output, png) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xarast-thumbnailer: {}: {e}", output.display());
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("xarast-thumbnailer: {}: {e}", input.display());
            ExitCode::FAILURE
        }
    }
}

/// The package's thumbnail, at most `size` pixels on the longer side.
fn thumbnail(input: &std::path::Path, size: u32) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(input).map_err(|e| e.to_string())?;
    let mut reader =
        XarastReader::open(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let png = reader
        .thumbnail()
        .map_err(|e| e.to_string())?
        .ok_or("the package has no thumbnail")?;
    // The same cap the writer enforces: decoding never allocates more than
    // 512 × 512 pixels whatever the file claims.
    let header = check_png(&png, THUMBNAIL_MAX_PX).map_err(str::to_owned)?;
    if header.width.max(header.height) <= size {
        return Ok(png);
    }
    let image = decode_png(&png).map_err(|e| e.to_string())?;
    let small = shrink(&image, size);
    encode_png(&small).map_err(|e| e.to_string())
}

/// Box-filters `s` down so its longer side is `max` (premultiplied, so the
/// average is right at transparent edges).
fn shrink(s: &Surface, max: u32) -> Surface {
    let (w, h) = (s.width(), s.height());
    let longer = w.max(h).max(1);
    let scale = |v: u32| ((u64::from(v) * u64::from(max)).div_ceil(u64::from(longer))) as u32;
    let (nw, nh) = (scale(w).clamp(1, max), scale(h).clamp(1, max));
    let mut out = Surface::new(nw, nh);
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for sy in y0..y1.min(h) {
                for sx in x0..x1.min(w) {
                    if let Some(p) = s.pixel(sx as i32, sy as i32) {
                        for (a, v) in acc.iter_mut().zip(p) {
                            *a += u32::from(v);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            let px = acc.map(|a| u8::try_from((a + n / 2) / n).unwrap_or(255));
            out.set_pixel(x as i32, y as i32, px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_is_shrunk_to_the_asked_size_and_keeps_its_shape() {
        let s = Surface::filled(256, 180, [10, 20, 30, 255]);
        let small = shrink(&s, 128);
        assert_eq!((small.width(), small.height()), (128, 90));
        assert_eq!(small.pixel(5, 5), Some([10, 20, 30, 255]));
        let tiny = shrink(&Surface::filled(256, 2, [0, 0, 0, 255]), 16);
        assert_eq!((tiny.width(), tiny.height()), (16, 1));
    }

    #[test]
    fn a_package_without_a_thumbnail_gives_none_and_a_saved_one_comes_out() {
        let dir = std::env::temp_dir().join(format!("xarast-thumbnailer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bare = dir.join("bare.xarast");
        xarast_format::save(
            &xarast_doc::Document::new_empty(),
            &bare,
            &xarast_format::SaveOptions::default(),
        )
        .unwrap();
        assert!(thumbnail(&bare, 256).is_err());

        let mut s = xarast_app::Session::new_empty(xarast_app::DocumentId(1));
        let with = dir.join("with.xarast");
        s.save_as(&with).unwrap();
        let full = thumbnail(&with, 256).unwrap();
        assert_eq!(
            check_png(&full, 512)
                .unwrap()
                .width
                .max(check_png(&full, 512).unwrap().height),
            256
        );
        let small = thumbnail(&with, 64).unwrap();
        let h = check_png(&small, 512).unwrap();
        assert_eq!(h.width.max(h.height), 64);
        assert!(thumbnail(&dir.join("missing.xarast"), 64).is_err());
        std::fs::write(dir.join("junk.xarast"), b"PK\x03\x04 not really").unwrap();
        assert!(thumbnail(&dir.join("junk.xarast"), 64).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
