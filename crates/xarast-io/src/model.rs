//! The export model: what is exported, at what size, over what background
//! (phase 11 T11.1.1, T11.1.2).
//!
//! These types carry no behaviour that needs the application: an
//! [`ExportArea`] is turned into a document rectangle by an
//! [`ExportSource`](crate::ExportSource), which the application implements.

use std::path::PathBuf;

use xarast_color::Rgba8;
use xarast_doc::NodeId;
use xarast_geom::{Mp, Point, Rect};
use xarast_render::RenderQuality;
use xarast_render::export::MAX_EXPORT_SIDE;

use crate::options::FormatOptions;

/// The largest pixel count an export may have: 2²⁹, a little over
/// 23 000 × 23 000. Formats that are not streamed hold the whole image.
pub const MAX_EXPORT_PIXELS: u64 = 1 << 29;

/// The lowest and highest resolution the sizing accepts, in dots per inch.
pub const DPI_RANGE: (f64, f64) = (1.0, 100_000.0);

/// What part of the document is exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportArea {
    /// The bounds of the selected objects.
    Selection,
    /// A page: `None` is the first page of the active spread.
    Page(Option<NodeId>),
    /// The active spread with its pasteboard margin.
    Spread,
    /// Everything drawn on the active spread's visible layers; the page
    /// when nothing is.
    Drawing,
    /// An explicit rectangle in document coordinates (millipoints, y up).
    Rect(Rect),
}

/// Which of pixel size, physical size and resolution is held fixed while
/// another is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizingAxis {
    /// The pixel size.
    Pixels,
    /// The physical (printed) size.
    Physical,
    /// The resolution.
    Dpi,
}

/// One edit of the sizing, as the dialog makes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizingEdit {
    /// A new resolution, in dots per inch.
    Dpi(f64),
    /// A new pixel width.
    PixelWidth(u32),
    /// A new pixel height.
    PixelHeight(u32),
    /// A new physical width.
    PhysicalWidth(Mp),
    /// A new physical height.
    PhysicalHeight(Mp),
}

impl SizingEdit {
    const fn axis(self) -> SizingAxis {
        match self {
            SizingEdit::Dpi(_) => SizingAxis::Dpi,
            SizingEdit::PixelWidth(_) | SizingEdit::PixelHeight(_) => SizingAxis::Pixels,
            SizingEdit::PhysicalWidth(_) | SizingEdit::PhysicalHeight(_) => SizingAxis::Physical,
        }
    }
}

/// Why a sizing is not usable.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum SizingError {
    /// The area has no width or no height.
    #[error("the export area is empty")]
    EmptyArea,
    /// A resolution outside [`DPI_RANGE`] or not finite.
    #[error("{0} dpi is not a usable resolution")]
    BadDpi(f64),
    /// A physical size of zero or less.
    #[error("the physical size must be positive")]
    BadPhysical,
    /// A pixel dimension of zero or over [`MAX_EXPORT_SIDE`], or more than
    /// [`MAX_EXPORT_PIXELS`] in total.
    #[error(
        "{width}x{height} px is outside 1..={MAX_EXPORT_SIDE} px a side or \
         {MAX_EXPORT_PIXELS} pixels"
    )]
    TooLarge {
        /// Width, possibly saturated.
        width: u64,
        /// Height, possibly saturated.
        height: u64,
    },
}

/// Pixel size, physical size and resolution, linked by
/// `pixels = physical × dpi`, with one of them pinned.
///
/// All arithmetic is `f64` on millipoints, and pixel counts are rounded
/// half away from zero, the renderer's rule (`research/03 §3.7`): 100 mm at
/// 300 dpi is 1181 px on every platform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportSizing {
    /// The quantity an edit of the other two leaves alone.
    pub pinned: SizingAxis,
    /// Width and height in pixels.
    pub pixels: (u32, u32),
    /// Width and height on paper. Zero means "the area's own size", which
    /// [`ExportSizing::resolve`] fills in.
    pub physical: (Mp, Mp),
    /// Dots per inch.
    pub dpi: f64,
    /// Keep width and height in the area's proportion.
    pub lock_aspect: bool,
}

impl Default for ExportSizing {
    /// 96 dpi at the area's own size: one screen pixel per pixel at 100 %.
    fn default() -> ExportSizing {
        ExportSizing::at_dpi(96.0)
    }
}

impl ExportSizing {
    /// The area at its own size and `dpi`, the resolution pinned.
    #[must_use]
    pub const fn at_dpi(dpi: f64) -> ExportSizing {
        ExportSizing {
            pinned: SizingAxis::Dpi,
            pixels: (0, 0),
            physical: (Mp::ZERO, Mp::ZERO),
            dpi,
            lock_aspect: true,
        }
    }

    /// An explicit pixel size, pinned. A zero dimension follows the area's
    /// aspect ratio; both zero is an error at [`ExportSizing::resolve`].
    #[must_use]
    pub const fn with_pixels(width: u32, height: u32) -> ExportSizing {
        ExportSizing {
            pinned: SizingAxis::Pixels,
            pixels: (width, height),
            physical: (Mp::ZERO, Mp::ZERO),
            dpi: 96.0,
            lock_aspect: true,
        }
    }

    /// Recomputes whatever follows from the pinned quantity for `area`,
    /// then validates.
    ///
    /// * Physical size zero becomes the area's size.
    /// * Pinned [`SizingAxis::Pixels`]: a zero pixel dimension follows the
    ///   aspect ratio, and the resolution follows from the width.
    /// * Otherwise the pixels follow from physical size × resolution.
    ///
    /// # Errors
    ///
    /// [`SizingError`] when any of the three is out of range.
    pub fn resolve(&mut self, area: Rect) -> Result<(), SizingError> {
        let (aw, ah) = area_size(area)?;
        if self.physical.0.raw() <= 0 || self.physical.1.raw() <= 0 {
            self.physical = (Mp::new(aw), Mp::new(ah));
        }
        match self.pinned {
            SizingAxis::Pixels => {
                let (pw, ph) = self.phys_f64();
                match self.pixels {
                    (0, 0) => {
                        return Err(SizingError::TooLarge {
                            width: 0,
                            height: 0,
                        });
                    }
                    (w, 0) => self.pixels.1 = px(f64::from(w) * ph / pw)?,
                    (0, h) => self.pixels.0 = px(f64::from(h) * pw / ph)?,
                    _ => {}
                }
                self.dpi = f64::from(self.pixels.0) * f64::from(Mp::PER_INCH) / pw;
            }
            SizingAxis::Dpi | SizingAxis::Physical => self.pixels_from_physical()?,
        }
        self.validate()
    }

    /// Applies one edit, keeping the pinned quantity and recomputing the
    /// third, then validates.
    ///
    /// Editing the pinned quantity itself is allowed: the resolution
    /// moves the pixels, the pixels move the resolution, and the physical
    /// size moves the pixels.
    ///
    /// # Errors
    ///
    /// [`SizingError`]; the sizing is left as the edit made it.
    pub fn edit(&mut self, edit: SizingEdit, area: Rect) -> Result<(), SizingError> {
        let (aw, ah) = area_size(area)?;
        if self.physical.0.raw() <= 0 || self.physical.1.raw() <= 0 {
            self.physical = (Mp::new(aw), Mp::new(ah));
        }
        let (pw, ph) = self.phys_f64();
        let aspect = ph / pw;
        match edit {
            SizingEdit::Dpi(d) => {
                check_dpi(d)?;
                self.dpi = d;
            }
            SizingEdit::PixelWidth(w) => {
                self.pixels.0 = w;
                if self.lock_aspect {
                    self.pixels.1 = px(f64::from(w) * aspect)?;
                }
            }
            SizingEdit::PixelHeight(h) => {
                self.pixels.1 = h;
                if self.lock_aspect {
                    self.pixels.0 = px(f64::from(h) / aspect)?;
                }
            }
            SizingEdit::PhysicalWidth(w) => {
                self.physical.0 = w;
                if self.lock_aspect {
                    self.physical.1 = Mp::from_f64_round(w.to_f64() * aspect);
                }
            }
            SizingEdit::PhysicalHeight(h) => {
                self.physical.1 = h;
                if self.lock_aspect {
                    self.physical.0 = Mp::from_f64_round(h.to_f64() / aspect);
                }
            }
        }
        if self.physical.0.raw() <= 0 || self.physical.1.raw() <= 0 {
            return Err(SizingError::BadPhysical);
        }
        let edited = edit.axis();
        let follower = if edited == self.pinned {
            match self.pinned {
                SizingAxis::Pixels => SizingAxis::Dpi,
                SizingAxis::Dpi | SizingAxis::Physical => SizingAxis::Pixels,
            }
        } else {
            third(self.pinned, edited)
        };
        match follower {
            SizingAxis::Pixels => self.pixels_from_physical()?,
            SizingAxis::Dpi => {
                let (pw, ph) = self.phys_f64();
                self.dpi = match edit {
                    SizingEdit::PixelHeight(_) => {
                        f64::from(self.pixels.1) * f64::from(Mp::PER_INCH) / ph
                    }
                    _ => f64::from(self.pixels.0) * f64::from(Mp::PER_INCH) / pw,
                };
            }
            SizingAxis::Physical => {
                check_dpi(self.dpi)?;
                let per_px = f64::from(Mp::PER_INCH) / self.dpi;
                self.physical = (
                    Mp::from_f64_round(f64::from(self.pixels.0) * per_px),
                    Mp::from_f64_round(f64::from(self.pixels.1) * per_px),
                );
            }
        }
        self.validate()
    }

    /// Checks all three quantities.
    ///
    /// # Errors
    ///
    /// [`SizingError`] for the first that is out of range.
    pub fn validate(&self) -> Result<(), SizingError> {
        check_dpi(self.dpi)?;
        if self.physical.0.raw() <= 0 || self.physical.1.raw() <= 0 {
            return Err(SizingError::BadPhysical);
        }
        let (w, h) = (u64::from(self.pixels.0), u64::from(self.pixels.1));
        let side = u64::from(MAX_EXPORT_SIDE);
        if w == 0 || h == 0 || w > side || h > side || w * h > MAX_EXPORT_PIXELS {
            return Err(SizingError::TooLarge {
                width: w,
                height: h,
            });
        }
        Ok(())
    }

    fn phys_f64(&self) -> (f64, f64) {
        (
            self.physical.0.to_f64().max(1.0),
            self.physical.1.to_f64().max(1.0),
        )
    }

    fn pixels_from_physical(&mut self) -> Result<(), SizingError> {
        check_dpi(self.dpi)?;
        let (pw, ph) = self.phys_f64();
        let k = self.dpi / f64::from(Mp::PER_INCH);
        self.pixels = (px(pw * k)?, px(ph * k)?);
        Ok(())
    }
}

/// Pixels from a real count: round half away from zero, at least one.
///
/// # Errors
///
/// When the count does not fit.
pub fn px(v: f64) -> Result<u32, SizingError> {
    let r = v.round();
    if !r.is_finite() || r > f64::from(MAX_EXPORT_SIDE) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let width = if r.is_finite() { r as u64 } else { u64::MAX };
        return Err(SizingError::TooLarge { width, height: 0 });
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok((r as u32).max(1))
}

fn check_dpi(d: f64) -> Result<(), SizingError> {
    if d.is_finite() && (DPI_RANGE.0..=DPI_RANGE.1).contains(&d) {
        Ok(())
    } else {
        Err(SizingError::BadDpi(d))
    }
}

fn third(a: SizingAxis, b: SizingAxis) -> SizingAxis {
    use SizingAxis::{Dpi, Physical, Pixels};
    match (a, b) {
        (Pixels, Physical) | (Physical, Pixels) => Dpi,
        (Pixels, Dpi) | (Dpi, Pixels) => Physical,
        _ => Pixels,
    }
}

fn area_size(area: Rect) -> Result<(i32, i32), SizingError> {
    if area.is_empty() || area.width().raw() <= 0 || area.height().raw() <= 0 {
        return Err(SizingError::EmptyArea);
    }
    Ok((area.width().raw(), area.height().raw()))
}

/// What the exported pixels are composited over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// Nothing: alpha is kept. A format or option without alpha flattens
    /// onto the paper colour instead and says so in the report.
    Transparent,
    /// The document's paper colour.
    Paper,
    /// An explicit colour; its alpha is honoured where the format has one.
    Colour(Rgba8),
}

impl Background {
    /// The straight RGBA the surface is cleared to, given the paper colour
    /// and whether the output keeps alpha. A background that is not opaque
    /// for an output without alpha is composited over the paper colour.
    #[must_use]
    pub fn clear_colour(self, paper: Rgba8, keeps_alpha: bool) -> [u8; 4] {
        let c = match self {
            Background::Transparent => Rgba8 { a: 0, ..paper },
            Background::Paper => paper,
            Background::Colour(c) => c,
        };
        if keeps_alpha || c.a == 255 {
            return [c.r, c.g, c.b, c.a];
        }
        let over = |s: u8, d: u8| {
            let (s, d, a) = (u32::from(s), u32::from(d), u32::from(c.a));
            #[allow(clippy::cast_possible_truncation)]
            let v = ((s * a + d * (255 - a) + 127) / 255) as u8;
            v
        };
        [
            over(c.r, paper.r),
            over(c.g, paper.g),
            over(c.b, paper.b),
            255,
        ]
    }
}

/// One export, fully described.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// What part of the document.
    pub area: ExportArea,
    /// Extra margin around the area, in document units.
    pub bleed: Mp,
    /// Pixel size, physical size and resolution.
    pub sizing: ExportSizing,
    /// What the pixels are composited over.
    pub background: Background,
    /// Render quality; [`RenderQuality::Final`] for every export the UI
    /// makes.
    pub quality: RenderQuality,
    /// The format and its options.
    pub options: FormatOptions,
    /// Where the file goes. Written atomically: a failed or cancelled
    /// export leaves nothing behind.
    pub destination: PathBuf,
}

impl ExportRequest {
    /// A request with the defaults: the drawing, 96 dpi, transparent,
    /// `Final`.
    #[must_use]
    pub fn new(options: FormatOptions, destination: PathBuf) -> ExportRequest {
        ExportRequest {
            area: ExportArea::Drawing,
            bleed: Mp::ZERO,
            sizing: ExportSizing::default(),
            background: Background::Transparent,
            quality: RenderQuality::Final,
            options,
            destination,
        }
    }

    /// The area rectangle grown by the bleed.
    ///
    /// # Errors
    ///
    /// [`SizingError::EmptyArea`] when the result is empty.
    pub fn bled(&self, area: Rect) -> Result<Rect, SizingError> {
        if area.is_empty() {
            return Err(SizingError::EmptyArea);
        }
        let b = self.bleed.raw().max(0);
        let r = Rect::new(
            Point::new(
                Mp::new(area.lo.x.raw().saturating_sub(b)),
                Mp::new(area.lo.y.raw().saturating_sub(b)),
            ),
            Point::new(
                Mp::new(area.hi.x.raw().saturating_add(b)),
                Mp::new(area.hi.y.raw().saturating_add(b)),
            ),
        );
        area_size(r)?;
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(mp: i32) -> Rect {
        Rect::raw(0, 0, mp, mp)
    }

    #[test]
    fn a_100_mm_square_at_300_dpi_is_1181_px() {
        let side = Mp::from_mm(100.0).raw();
        let mut s = ExportSizing::at_dpi(300.0);
        s.resolve(square(side)).unwrap();
        assert_eq!(s.pixels, (1181, 1181));
    }

    #[test]
    fn twenty_triples_including_halves() {
        // (area side in millipoints, dpi, expected pixels)
        let table: [(i32, f64, u32); 20] = [
            (72_000, 96.0, 96),
            (72_000, 72.0, 72),
            (72_000, 300.0, 300),
            (72_000, 1.5, 2),       // exactly .5 rounds away from zero
            (72_000, 2.5, 3),       // and not to even
            (36_000, 3.0, 2),       // 1.5 px
            (144_000, 150.25, 301), // 300.5 px
            (36_000, 1.0, 1),       // 0.5 px, and never below one
            (1, 96.0, 1),
            (Mp::from_mm(100.0).raw(), 300.0, 1181),
            (Mp::from_mm(210.0).raw(), 300.0, 2480),
            (Mp::from_mm(297.0).raw(), 300.0, 3508),
            (Mp::from_mm(25.4).raw(), 600.0, 600),
            (Mp::from_mm(10.0).raw(), 96.0, 38),
            (720_000, 96.0, 960),
            (720_000, 1200.0, 12_000),
            (100_000, 72.0, 100),
            (100_500, 72.0, 101), // 100.5
            (99_500, 72.0, 100),  // 99.5
            (48_000, 108.0, 72),
        ];
        for (side, dpi, want) in table {
            let mut s = ExportSizing::at_dpi(dpi);
            s.resolve(square(side)).unwrap();
            assert_eq!(s.pixels, (want, want), "{side} mp at {dpi} dpi");
        }
    }

    #[test]
    fn pinned_pixels_follow_the_aspect_and_set_the_dpi() {
        let area = Rect::raw(0, 0, 72_000, 144_000);
        let mut s = ExportSizing::with_pixels(300, 0);
        s.resolve(area).unwrap();
        assert_eq!(s.pixels, (300, 600));
        assert!((s.dpi - 300.0).abs() < 1e-9);
        let mut s = ExportSizing::with_pixels(0, 0);
        assert!(s.resolve(area).is_err());
    }

    #[test]
    fn edits_move_the_follower_and_keep_the_pin() {
        let area = Rect::raw(0, 0, 72_000, 36_000);
        // Resolution pinned: a new pixel width moves the physical size.
        let mut s = ExportSizing::at_dpi(100.0);
        s.resolve(area).unwrap();
        assert_eq!(s.pixels, (100, 50));
        s.edit(SizingEdit::PixelWidth(200), area).unwrap();
        assert_eq!(s.pixels, (200, 100));
        assert!((s.dpi - 100.0).abs() < 1e-9);
        assert_eq!(s.physical, (Mp::new(144_000), Mp::new(72_000)));
        // Physical pinned: a new resolution moves the pixels.
        s.pinned = SizingAxis::Physical;
        s.edit(SizingEdit::Dpi(50.0), area).unwrap();
        assert_eq!(s.pixels, (100, 50));
        // Physical pinned: new pixels move the resolution.
        s.edit(SizingEdit::PixelHeight(100), area).unwrap();
        assert_eq!(s.pixels, (200, 100));
        assert!((s.dpi - 100.0).abs() < 1e-9);
        // Pixels pinned: a new physical width moves the resolution.
        s.pinned = SizingAxis::Pixels;
        s.edit(SizingEdit::PhysicalWidth(Mp::new(72_000)), area)
            .unwrap();
        assert_eq!(s.pixels, (200, 100));
        assert_eq!(s.physical.1, Mp::new(36_000));
        assert!((s.dpi - 200.0).abs() < 1e-9);
        // Pixels pinned, resolution edited: the physical size follows.
        s.edit(SizingEdit::Dpi(400.0), area).unwrap();
        assert_eq!(s.physical, (Mp::new(36_000), Mp::new(18_000)));
    }

    #[test]
    fn nonsense_is_refused() {
        let area = square(72_000);
        let mut s = ExportSizing::at_dpi(f64::NAN);
        assert!(matches!(s.resolve(area), Err(SizingError::BadDpi(_))));
        let mut s = ExportSizing::at_dpi(96.0);
        assert!(matches!(
            s.resolve(Rect::EMPTY),
            Err(SizingError::EmptyArea)
        ));
        let mut s = ExportSizing::at_dpi(99_999.0);
        assert!(matches!(
            s.resolve(square(72_000 * 100)),
            Err(SizingError::TooLarge { .. })
        ));
        let mut s = ExportSizing::with_pixels(40_000, 40_000);
        assert!(s.resolve(area).is_err(), "over the pixel budget");
    }

    #[test]
    fn backgrounds_flatten_only_without_alpha() {
        let paper = Rgba8::WHITE;
        assert_eq!(
            Background::Transparent.clear_colour(paper, true),
            [255, 255, 255, 0]
        );
        assert_eq!(
            Background::Transparent.clear_colour(paper, false),
            [255, 255, 255, 255]
        );
        let half_red = Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 128,
        };
        assert_eq!(
            Background::Colour(half_red).clear_colour(paper, false),
            [255, 127, 127, 255]
        );
        assert_eq!(
            Background::Colour(half_red).clear_colour(paper, true),
            [255, 0, 0, 128]
        );
    }

    #[test]
    fn bleed_grows_the_area() {
        let mut r = ExportRequest::new(FormatOptions::default(), PathBuf::from("x.png"));
        r.bleed = Mp::new(1000);
        let b = r.bled(square(72_000)).unwrap();
        assert_eq!(b, Rect::raw(-1000, -1000, 73_000, 73_000));
    }
}
