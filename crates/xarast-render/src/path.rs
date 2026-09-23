//! The renderer's view of a path: an `xarast-geom` [`Path`] with its `f64`
//! curve form and its bounds computed once and shared.
//!
//! Paths arrive in document space (integer millipoints) and are converted to
//! `f64` here, which is exact — an `i32` fits in a double with 21 bits to
//! spare. The conversion to `f32` happens much later and only relative to a
//! tile; see [`crate::precision`].

use std::sync::Arc;

use kurbo::{BezPath, PathEl, Shape};
use xarast_geom::Path;

/// A shared path, ready to hand to a rasteriser.
#[derive(Debug, Clone)]
pub struct PathRef {
    path: Arc<Path>,
    bez: Arc<BezPath>,
    bounds: kurbo::Rect,
    polyline: bool,
}

impl PartialEq for PathRef {
    fn eq(&self, other: &PathRef) -> bool {
        Arc::ptr_eq(&self.path, &other.path) || self.path == other.path
    }
}

impl PathRef {
    /// Adopts a document-space path.
    #[must_use]
    pub fn new(path: Path) -> PathRef {
        PathRef::from_arc(Arc::new(path))
    }

    /// Adopts an already shared path.
    #[must_use]
    pub fn from_arc(path: Arc<Path>) -> PathRef {
        let bez = path.to_bez_path();
        let bounds = bez.bounding_box();
        let polyline = bez
            .elements()
            .iter()
            .all(|el| !matches!(el, PathEl::QuadTo(..) | PathEl::CurveTo(..)));
        PathRef {
            path,
            bez: Arc::new(bez),
            bounds,
            polyline,
        }
    }

    /// The document-space path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `f64` curve form, in document units.
    #[must_use]
    pub fn bez(&self) -> &BezPath {
        &self.bez
    }

    /// The control-point bounding box, in document units.
    ///
    /// Control-point bounds, not tight bounds: they are cheap, they are
    /// conservative, and culling only needs a superset.
    #[must_use]
    pub fn bounds(&self) -> kurbo::Rect {
        self.bounds
    }

    /// Whether the path has only straight segments, so that flattening it
    /// would reproduce it unchanged.
    #[must_use]
    pub fn is_polyline(&self) -> bool {
        self.polyline
    }

    /// Whether the path draws nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_geom::{Mp, Point, Rect};

    #[test]
    fn a_rectangle_keeps_its_bounds_through_the_conversion() {
        let mut b = Path::builder();
        b.rect(Rect::new(
            Point::new(Mp::from_pt(1.0), Mp::from_pt(2.0)),
            Point::new(Mp::from_pt(3.0), Mp::from_pt(5.0)),
        ));
        let p = PathRef::new(b.build());
        let r = p.bounds();
        assert!((r.x0 - 1000.0).abs() < 1e-9);
        assert!((r.x1 - 3000.0).abs() < 1e-9);
        assert!(!p.is_empty());
    }

    #[test]
    fn an_empty_path_is_empty() {
        assert!(PathRef::new(Path::new()).is_empty());
    }
}
