//! EXIF orientation, applied once at import so that everything downstream
//! sees upright pixels.

/// The eight EXIF orientations (TIFF tag 0x0112), named for the transform
/// that turns the *stored* pixels into the *displayed* ones.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Orientation {
    /// 1: stored upright.
    #[default]
    Normal,
    /// 2: mirror left–right.
    FlipH,
    /// 3: rotate 180°.
    Rot180,
    /// 4: mirror top–bottom.
    FlipV,
    /// 5: mirror along the main diagonal (x ↔ y).
    Transpose,
    /// 6: rotate 90° clockwise.
    Rot90,
    /// 7: mirror along the anti-diagonal.
    Transverse,
    /// 8: rotate 90° counter-clockwise (270° clockwise).
    Rot270,
}

impl Orientation {
    /// From the EXIF value 1–8; anything else is `None`.
    #[must_use]
    pub const fn from_exif(v: u8) -> Option<Orientation> {
        Some(match v {
            1 => Orientation::Normal,
            2 => Orientation::FlipH,
            3 => Orientation::Rot180,
            4 => Orientation::FlipV,
            5 => Orientation::Transpose,
            6 => Orientation::Rot90,
            7 => Orientation::Transverse,
            8 => Orientation::Rot270,
            _ => return None,
        })
    }

    /// The EXIF value 1–8.
    #[must_use]
    pub const fn to_exif(self) -> u8 {
        match self {
            Orientation::Normal => 1,
            Orientation::FlipH => 2,
            Orientation::Rot180 => 3,
            Orientation::FlipV => 4,
            Orientation::Transpose => 5,
            Orientation::Rot90 => 6,
            Orientation::Transverse => 7,
            Orientation::Rot270 => 8,
        }
    }

    /// Whether applying it swaps width and height.
    #[must_use]
    pub const fn swaps_axes(self) -> bool {
        matches!(
            self,
            Orientation::Transpose
                | Orientation::Rot90
                | Orientation::Transverse
                | Orientation::Rot270
        )
    }

    /// Where the displayed pixel `(dx, dy)` comes from in the stored image
    /// of size `w × h`.
    #[inline]
    fn source(self, dx: usize, dy: usize, w: usize, h: usize) -> (usize, usize) {
        match self {
            Orientation::Normal => (dx, dy),
            Orientation::FlipH => (w - 1 - dx, dy),
            Orientation::Rot180 => (w - 1 - dx, h - 1 - dy),
            Orientation::FlipV => (dx, h - 1 - dy),
            Orientation::Transpose => (dy, dx),
            Orientation::Rot90 => (dy, h - 1 - dx),
            Orientation::Transverse => (w - 1 - dy, h - 1 - dx),
            Orientation::Rot270 => (w - 1 - dy, dx),
        }
    }
}

/// Applies `o` to a `w × h` RGBA8 buffer, returning the upright pixels and
/// their dimensions. `Normal` returns the input untouched, without a copy.
#[must_use]
pub fn apply(o: Orientation, w: u32, h: u32, pixels: Box<[u8]>) -> (u32, u32, Box<[u8]>) {
    if o == Orientation::Normal || w == 0 || h == 0 {
        return (w, h, pixels);
    }
    let (sw, sh) = (w as usize, h as usize);
    let (dw, dh) = if o.swaps_axes() { (sh, sw) } else { (sw, sh) };
    let mut out = vec![0u8; pixels.len()].into_boxed_slice();
    for dy in 0..dh {
        for dx in 0..dw {
            let (sx, sy) = o.source(dx, dy, sw, sh);
            let s = (sy * sw + sx) * 4;
            let d = (dy * dw + dx) * 4;
            out[d..d + 4].copy_from_slice(&pixels[s..s + 4]);
        }
    }
    let (ow, oh) = if o.swaps_axes() { (h, w) } else { (w, h) };
    (ow, oh, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 × 2 image whose pixels are numbered 0..6 in the red channel.
    fn numbered() -> Box<[u8]> {
        (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect()
    }

    fn reds(b: &[u8]) -> Vec<u8> {
        b.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
    }

    #[test]
    fn each_orientation_moves_pixels_as_exif_defines() {
        // Stored:      0 1 2
        //              3 4 5
        let cases = [
            (Orientation::Normal, (3, 2), vec![0, 1, 2, 3, 4, 5]),
            (Orientation::FlipH, (3, 2), vec![2, 1, 0, 5, 4, 3]),
            (Orientation::Rot180, (3, 2), vec![5, 4, 3, 2, 1, 0]),
            (Orientation::FlipV, (3, 2), vec![3, 4, 5, 0, 1, 2]),
            (Orientation::Transpose, (2, 3), vec![0, 3, 1, 4, 2, 5]),
            (Orientation::Rot90, (2, 3), vec![3, 0, 4, 1, 5, 2]),
            (Orientation::Transverse, (2, 3), vec![5, 2, 4, 1, 3, 0]),
            (Orientation::Rot270, (2, 3), vec![2, 5, 1, 4, 0, 3]),
        ];
        for (o, dims, want) in cases {
            let (w, h, out) = apply(o, 3, 2, numbered());
            assert_eq!((w, h), dims, "{o:?}");
            assert_eq!(reds(&out), want, "{o:?}");
        }
    }

    #[test]
    fn exif_values_round_trip() {
        for v in 1..=8 {
            assert_eq!(Orientation::from_exif(v).map(Orientation::to_exif), Some(v));
        }
        assert_eq!(Orientation::from_exif(0), None);
        assert_eq!(Orientation::from_exif(9), None);
    }
}
