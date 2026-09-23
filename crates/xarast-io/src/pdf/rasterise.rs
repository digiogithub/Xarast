//! The fidelity ladder's last step: render an object to an image.
//!
//! Two modes, both on the deterministic CPU backend through
//! [`ListRasteriser`]:
//!
//! * **Object** — the object alone, over transparency. Right when only the
//!   object's *paint* is inexpressible (a perspective gradient, a bitmap
//!   fill, graduated transparency in the ordinary mixing family): the
//!   image's alpha carries the object's coverage and opacity, and PDF's
//!   ordinary compositing puts it on the page exactly.
//! * **Backdrop** — the object *and everything drawn before it* over the
//!   object's bounds. Right when the object's *blend* is inexpressible
//!   (Stained Glass, Bleach, Contrast, …): the blend reads the backdrop,
//!   so the backdrop has to be in the image. The vector content under the
//!   image stays in the file; the image covers it within the rectangle.
//!
//! Both select commands from a display list built for the image's pixel
//! grid, matching the object by its scene op, so the pixels are exactly
//! what the raster exporters draw for the same region.

use kurbo::Rect;
use xarast_render::export::ListRasteriser;
use xarast_render::{
    BlendFamily, DeviceRect, DirtyRect, DisplayList, DrawCmd, RenderQuality, Resolver, Scene,
    Transform2D, ViewParams,
};

use crate::report::ExportError;

/// The most pixels one rasterised object may have: 6000 × 6000. A larger
/// object is rendered at a lower resolution, which the report states.
pub const MAX_RASTER_PIXELS: f64 = 36.0e6;

/// Which object to rasterise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A fill, stroke or image: its scene op.
    Op(u32),
    /// A layer: the scene op of its `PushLayer`, which its `PopLayer`
    /// names.
    Layer(u32),
}

/// A rendered object, ready to place.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    /// Where it goes, in page points: `x0, y0, x1, y1`, y up.
    pub rect: [f64; 4],
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// Straight RGB, rows top to bottom.
    pub rgb: Vec<u8>,
    /// Alpha, when any pixel is not opaque.
    pub alpha: Option<Vec<u8>>,
    /// The resolution actually used.
    pub dpi: f64,
}

/// Renders objects of one scene for one page.
#[derive(Debug)]
pub struct Rasteriser<'a> {
    /// The scene.
    pub scene: &'a Scene,
    /// Its ramps and images.
    pub resolver: &'a Resolver,
    /// Document to page points.
    pub to_page: Transform2D,
    /// The page, in points.
    pub page: Rect,
    /// The requested resolution.
    pub dpi: f64,
    /// Render quality.
    pub quality: RenderQuality,
    /// What a backdrop render starts from: the page background.
    pub clear: [u8; 4],
    lr: ListRasteriser,
}

impl<'a> Rasteriser<'a> {
    /// A rasteriser for a page.
    #[must_use]
    pub fn new(
        scene: &'a Scene,
        resolver: &'a Resolver,
        to_page: Transform2D,
        page: Rect,
        dpi: f64,
        quality: RenderQuality,
        clear: [u8; 4],
    ) -> Rasteriser<'a> {
        Rasteriser {
            scene,
            resolver,
            to_page,
            page,
            dpi,
            quality,
            clear,
            lr: ListRasteriser::new(),
        }
    }

    /// Renders `target` over `bounds` (page points), alone or with its
    /// backdrop. `None` when nothing lands on the page.
    ///
    /// # Errors
    ///
    /// `Cancelled`, or `Render` when the backend refuses.
    pub fn render(
        &mut self,
        bounds: DeviceRect,
        target: Target,
        backdrop: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<Raster>, ExportError> {
        let r = Rect::new(
            f64::from(bounds.x0),
            f64::from(bounds.y0),
            f64::from(bounds.x1),
            f64::from(bounds.y1),
        )
        .intersect(self.page);
        if r.width() <= 0.0 || r.height() <= 0.0 {
            return Ok(None);
        }
        let mut s = self.dpi / 72.0;
        let px = |s: f64| {
            (
                (r.width() * s).ceil().max(1.0),
                (r.height() * s).ceil().max(1.0),
            )
        };
        let (w, h) = px(s);
        if w * h > MAX_RASTER_PIXELS {
            s *= (MAX_RASTER_PIXELS / (w * h)).sqrt();
        }
        let side = f64::from(xarast_render::export::MAX_EXPORT_SIDE);
        let (w, h) = px(s);
        if w > side || h > side {
            s *= side / w.max(h);
        }
        let (w, h) = px(s);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (width, height) = (w as u32, h as u32);
        // Page points, y up, to the image grid, y down, with the image's
        // top-left at the rectangle's top-left.
        let to_px = Transform2D::new([s, 0.0, 0.0, -s, -r.x0 * s, r.y1 * s]);
        let view = ViewParams {
            transform: self.to_page.then(to_px),
            viewport: DeviceRect::from_size(width, height),
            quality: self.quality,
            dpi: s * 72.0,
        };
        let dl = DisplayList::build(self.scene, &view, &DirtyRect::of(view.viewport));
        let Some(cmds) = select(dl.commands(), target, backdrop) else {
            return Ok(None);
        };
        let sub = dl.with_commands(cmds);
        let mut render = |clear: [u8; 4]| {
            self.lr
                .render(&sub, self.resolver, width, height, clear, cancelled)
                .map_err(|e| match e {
                    xarast_render::export::ExportRenderError::Cancelled => ExportError::Cancelled,
                    other => ExportError::Render(other.to_string()),
                })
        };
        let n = width as usize * height as usize;
        let mut rgb = Vec::with_capacity(n * 3);
        let mut alpha = Vec::with_capacity(n);
        if backdrop {
            let surface = render(self.clear)?;
            for p in surface.data().as_chunks::<4>().0 {
                let a = p[3];
                rgb.extend_from_slice(&[
                    unpremultiply(p[0], a),
                    unpremultiply(p[1], a),
                    unpremultiply(p[2], a),
                ]);
                alpha.push(a);
            }
        } else {
            // The object over opaque black and over opaque white. Mixing is
            // linear in the backdrop, so the two give the object's coverage
            // and its premultiplied colour exactly, whatever the renderer
            // does over a transparent destination (it mixes towards the
            // straight colour stored there, black: XARA-T follow-up in
            // `docs/memory/export.md`).
            let black = render([0, 0, 0, 255])?;
            let white = render([255, 255, 255, 255])?;
            for (b, w) in black
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(white.data().as_chunks::<4>().0)
            {
                let spread: u32 = (0..3).map(|k| u32::from(w[k].saturating_sub(b[k]))).sum();
                let a = u8::try_from(255 - (spread + 1) / 3).unwrap_or(0);
                rgb.extend_from_slice(&[
                    unpremultiply(b[0], a),
                    unpremultiply(b[1], a),
                    unpremultiply(b[2], a),
                ]);
                alpha.push(a);
            }
        }
        if alpha.iter().all(|&a| a == 0) {
            return Ok(None);
        }
        let alpha = if alpha.iter().all(|&a| a == 255) {
            None
        } else {
            Some(alpha)
        };
        Ok(Some(Raster {
            rect: [r.x0, r.y1 - h / s, r.x0 + w / s, r.y1],
            width,
            height,
            rgb,
            alpha,
            dpi: s * 72.0,
        }))
    }
}

/// A premultiplied channel to straight.
fn unpremultiply(c: u8, a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    let v = (u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a);
    u8::try_from(v.min(255)).unwrap_or(255)
}

/// The commands that draw `target`: the target alone, or every command up
/// to and including it with the clips and layers still open closed.
/// `None` when the target is not in the list (it misses the image).
fn select(cmds: &[DrawCmd], target: Target, backdrop: bool) -> Option<Vec<DrawCmd>> {
    let is_target = |c: &DrawCmd| match (target, c) {
        (Target::Op(op), c) => {
            !matches!(c, DrawCmd::PushClip { .. }) && DisplayList::op_of(c) == Some(op)
        }
        (Target::Layer(l), DrawCmd::PopLayer { layer, .. }) => *layer == l,
        _ => false,
    };
    if !backdrop {
        return match target {
            Target::Op(_) => cmds.iter().find(|c| is_target(c)).map(|c| vec![*c]),
            // A layer alone is its commands from the push to the pop; a
            // layer is only ever rasterised with its backdrop.
            Target::Layer(_) => None,
        };
    }
    #[derive(Clone, Copy)]
    enum Open {
        Clip,
        Layer,
    }
    let mut out = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut found = false;
    for c in cmds {
        out.push(*c);
        match c {
            DrawCmd::PushClip { .. } => open.push(Open::Clip),
            DrawCmd::PushLayer { .. } => open.push(Open::Layer),
            DrawCmd::PopClip | DrawCmd::PopLayer { .. } => {
                open.pop();
            }
            _ => {}
        }
        if is_target(c) {
            found = true;
            break;
        }
    }
    if !found {
        return None;
    }
    for o in open.into_iter().rev() {
        out.push(match o {
            Open::Clip => DrawCmd::PopClip,
            // Unmatched: composited opaquely. The layer's own opacity is
            // applied by the PDF group the image is placed in.
            Open::Layer => DrawCmd::PopLayer {
                blend: BlendFamily::Mix,
                layer: u32::MAX,
            },
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(op: u32) -> DrawCmd {
        DrawCmd::Fill {
            op,
            xf: 0,
            paint: 0,
            transp: 0,
            dst_read: false,
            bounds: DeviceRect::from_size(1, 1),
        }
    }

    #[test]
    fn a_backdrop_prefix_is_balanced() {
        let cmds = [
            fill(0),
            DrawCmd::PushClip { op: 1, xf: 0 },
            fill(2),
            fill(3),
            DrawCmd::PopClip,
            fill(5),
        ];
        let got = select(&cmds, Target::Op(3), true).unwrap();
        assert_eq!(got.len(), 5);
        assert_eq!(got[4], DrawCmd::PopClip);
        let alone = select(&cmds, Target::Op(3), false).unwrap();
        assert_eq!(alone, vec![fill(3)]);
        // A clip's op is never the target.
        assert!(select(&cmds, Target::Op(1), false).is_none());
        assert!(select(&cmds, Target::Op(9), true).is_none());
    }
}
