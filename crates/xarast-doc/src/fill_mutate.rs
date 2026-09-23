//! Changing a fill's shape while keeping its values: `MutateFill` (phase 8,
//! T8.2.5).
//!
//! The point mapping is the table fixed in
//! `docs/phases/phase-08-colour-fills-transparency.md` §W8.2:
//!
//! - any → **flat**: take `from`;
//! - flat → any gradient: `from` = the flat value, `to` = the caller's
//!   `flat_end` (for a colour, the same colour at zero saturation), control
//!   points on the object's bounding box — its diagonal for a linear fill, the
//!   inscribed circle for radial, conical and diamond fills, its corners for a
//!   three- or four-colour fill;
//! - linear ↔ radial ↔ conical ↔ diamond: `centre := start`, `major := end`,
//!   `minor := start + perp(end − start)`, keeping `from`, `to`, the ramp and
//!   the profile;
//! - to three/four colour: the ramp is dropped (those shapes carry none), the
//!   third and fourth values seeded from the ramp's middle stop when there is
//!   one, else from `to`; from three/four colour: `from` = corner 0, `to` =
//!   corner 1, the arm = origin → axis 1.
//!
//! Bitmap, fractal and noise fills are not mutated (their phases own them).

use xarast_color::{Colour, ColourValue, Stop, TranspMode, Transparency};
use xarast_geom::{Point, Rect, Vector};

use crate::Document;
use crate::fill::{FillGeometry, Ramp};
use crate::fill_edit::{FillChannel, FillValue, PaintSlot, fill_in_force, set_own_attr};
use crate::history::{Command, EditError, Tx};
use crate::tree::NodeId;

/// The shapes a fill can be mutated between: the fill tool's type menu.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FillShape {
    /// One value everywhere.
    Flat,
    /// A linear gradient.
    Linear,
    /// A radial gradient kept circular.
    Circular,
    /// A radial gradient with free axes.
    Elliptical,
    /// A conical sweep.
    Conical,
    /// A diamond (square) gradient.
    Diamond,
    /// A three-colour fill.
    ThreeColour,
    /// A four-colour fill.
    FourColour,
}

impl FillShape {
    /// Every shape, in the order the type menu lists them.
    pub const ALL: [FillShape; 8] = [
        FillShape::Flat,
        FillShape::Linear,
        FillShape::Circular,
        FillShape::Elliptical,
        FillShape::Conical,
        FillShape::Diamond,
        FillShape::ThreeColour,
        FillShape::FourColour,
    ];

    /// The name the menu shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            FillShape::Flat => "Flat",
            FillShape::Linear => "Linear",
            FillShape::Circular => "Circular",
            FillShape::Elliptical => "Elliptical",
            FillShape::Conical => "Conical",
            FillShape::Diamond => "Diamond",
            FillShape::ThreeColour => "Three colour",
            FillShape::FourColour => "Four colour",
        }
    }

    /// The shape of a fill, or `None` for a bitmap, fractal or noise fill.
    #[must_use]
    pub fn of<S: Stop>(g: &FillGeometry<S>) -> Option<FillShape> {
        Some(match g {
            FillGeometry::Flat { .. } => FillShape::Flat,
            FillGeometry::Linear { .. } => FillShape::Linear,
            FillGeometry::Radial {
                aspect_locked: true,
                ..
            } => FillShape::Circular,
            FillGeometry::Radial { .. } => FillShape::Elliptical,
            FillGeometry::Conical { .. } => FillShape::Conical,
            FillGeometry::Diamond { .. } => FillShape::Diamond,
            FillGeometry::ThreeColour { .. } => FillShape::ThreeColour,
            FillGeometry::FourColour { .. } => FillShape::FourColour,
            FillGeometry::Bitmap { .. }
            | FillGeometry::Fractal { .. }
            | FillGeometry::Noise { .. } => return None,
        })
    }
}

/// What a mutation could not carry over.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct MutationLoss {
    /// Intermediate stops were dropped (the target shape has no ramp).
    pub ramp_dropped: bool,
    /// A control point had no counterpart and was derived instead.
    pub points_approximated: bool,
}

fn perp(v: Vector) -> Vector {
    Vector::new(v.dy.saturating_neg(), v.dx)
}

/// The values a fill carries, in shape-neutral form.
struct Values<S: Stop> {
    from: S,
    to: S,
    ramp: Ramp<S>,
    extra: Option<(S, Option<S>)>,
}

/// Mutates `g` into `to`. `bounds` is the object's bounding box, used when
/// the old fill has no control points to map (a flat fill); `flat_end` is
/// the far value a flat fill's new gradient gets.
///
/// # Errors
///
/// A bitmap, fractal or noise source.
pub fn mutate_fill<S: Stop>(
    g: &FillGeometry<S>,
    to: FillShape,
    bounds: Rect,
    flat_end: S,
) -> Result<(FillGeometry<S>, MutationLoss), &'static str> {
    let from_shape = FillShape::of(g).ok_or("this fill cannot change type")?;
    let mut loss = MutationLoss::default();
    if from_shape == to {
        return Ok((g.clone(), loss));
    }
    // The values and, when the source has one, its arm.
    let (values, arm, third): (Values<S>, Option<(Point, Point)>, Option<Point>) = match g {
        FillGeometry::Flat { value } => (
            Values {
                from: value.clone(),
                to: flat_end,
                ramp: Ramp::new(),
                extra: None,
            },
            None,
            None,
        ),
        FillGeometry::Linear {
            start,
            end,
            from,
            to,
            ramp,
            ..
        } => (
            Values {
                from: from.clone(),
                to: to.clone(),
                ramp: ramp.clone(),
                extra: None,
            },
            Some((*start, *end)),
            None,
        ),
        FillGeometry::Radial {
            centre,
            major,
            minor,
            from,
            to,
            ramp,
            ..
        } => (
            Values {
                from: from.clone(),
                to: to.clone(),
                ramp: ramp.clone(),
                extra: None,
            },
            Some((*centre, *major)),
            Some(*minor),
        ),
        FillGeometry::Conical {
            centre,
            zero_dir,
            from,
            to,
            ramp,
        } => (
            Values {
                from: from.clone(),
                to: to.clone(),
                ramp: ramp.clone(),
                extra: None,
            },
            Some((*centre, *zero_dir)),
            None,
        ),
        FillGeometry::Diamond {
            centre,
            corner1,
            corner2,
            from,
            to,
            ramp,
            ..
        } => (
            Values {
                from: from.clone(),
                to: to.clone(),
                ramp: ramp.clone(),
                extra: None,
            },
            Some((*centre, *corner1)),
            Some(*corner2),
        ),
        FillGeometry::ThreeColour {
            origin,
            axis1,
            axis2,
            c0,
            c1,
            c2,
        } => (
            Values {
                from: c0.clone(),
                to: c1.clone(),
                ramp: Ramp::new(),
                extra: Some((c2.clone(), None)),
            },
            Some((*origin, *axis1)),
            Some(*axis2),
        ),
        FillGeometry::FourColour {
            origin,
            axis1,
            axis2,
            c0,
            c1,
            c2,
            c3,
            ..
        } => (
            Values {
                from: c0.clone(),
                to: c1.clone(),
                ramp: Ramp::new(),
                extra: Some((c2.clone(), Some(c3.clone()))),
            },
            Some((*origin, *axis1)),
            Some(*axis2),
        ),
        _ => return Err("this fill cannot change type"),
    };
    let Values {
        from,
        to: to_value,
        ramp,
        extra,
    } = values;

    let out = match to {
        FillShape::Flat => FillGeometry::Flat { value: from },
        FillShape::ThreeColour | FillShape::FourColour => {
            if !ramp.is_empty() {
                loss.ramp_dropped = true;
            }
            let seed = ramp
                .stops()
                .get(ramp.stops().len() / 2)
                .map_or_else(|| to_value.clone(), |s| s.value.clone());
            let (c2, c3) = match extra {
                Some((c2, c3)) => (c2, c3.unwrap_or_else(|| to_value.clone())),
                None => (seed.clone(), seed),
            };
            let (origin, axis1, axis2) = match arm {
                Some((a, b)) => (a, b, third.unwrap_or_else(|| a + perp(b - a))),
                None => (
                    bounds.lo,
                    Point::new(bounds.hi.x, bounds.lo.y),
                    Point::new(bounds.lo.x, bounds.hi.y),
                ),
            };
            if to == FillShape::ThreeColour {
                FillGeometry::ThreeColour {
                    origin,
                    axis1,
                    axis2,
                    c0: from,
                    c1: to_value,
                    c2,
                }
            } else {
                FillGeometry::FourColour {
                    origin,
                    axis1,
                    axis2,
                    axis3: axis1 + (axis2 - origin),
                    c0: from,
                    c1: to_value,
                    c2,
                    c3,
                }
            }
        }
        FillShape::Linear
        | FillShape::Circular
        | FillShape::Elliptical
        | FillShape::Conical
        | FillShape::Diamond => {
            if matches!(
                from_shape,
                FillShape::ThreeColour | FillShape::FourColour | FillShape::Elliptical
            ) || (from_shape == FillShape::Diamond && to != FillShape::Elliptical)
            {
                loss.points_approximated = true;
            }
            let (a, b) = arm.unwrap_or_else(|| flat_arm(to, bounds));
            let side = a + perp(b - a);
            match to {
                FillShape::Linear => FillGeometry::Linear {
                    start: a,
                    end: b,
                    persp: None,
                    from,
                    to: to_value,
                    ramp,
                },
                FillShape::Circular | FillShape::Elliptical => FillGeometry::Radial {
                    centre: a,
                    major: b,
                    minor: if to == FillShape::Elliptical {
                        third
                            .filter(|_| from_shape == FillShape::Diamond)
                            .unwrap_or(side)
                    } else {
                        side
                    },
                    aspect_locked: to == FillShape::Circular,
                    persp: None,
                    from,
                    to: to_value,
                    ramp,
                },
                FillShape::Conical => FillGeometry::Conical {
                    centre: a,
                    zero_dir: b,
                    from,
                    to: to_value,
                    ramp,
                },
                _ => FillGeometry::Diamond {
                    centre: a,
                    corner1: b,
                    corner2: third
                        .filter(|_| from_shape == FillShape::Elliptical)
                        .unwrap_or(side),
                    persp: None,
                    from,
                    to: to_value,
                    ramp,
                },
            }
        }
    };
    Ok((out, loss))
}

/// A flat fill's new arm: the bounding box's diagonal for a linear fill,
/// the inscribed circle's centre and radius otherwise.
fn flat_arm(to: FillShape, bounds: Rect) -> (Point, Point) {
    if bounds.is_empty() {
        let p = Point::ORIGIN;
        return (p, p + Vector::raw(72_000, 0));
    }
    if to == FillShape::Linear {
        return (bounds.lo, bounds.hi);
    }
    let c = bounds.centre();
    let r = bounds.width().raw().min(bounds.height().raw()) / 2;
    (c, c + Vector::raw(r.max(1), 0))
}

/// The far value a flat colour's new gradient gets: the same colour at zero
/// saturation, or — when it has none already — black or white, whichever
/// contrasts.
#[must_use]
pub fn desaturated(doc: &Document, c: &Colour) -> Colour {
    let v = c.resolve(&doc.resources.colours).to_hsvt();
    let ColourValue::Hsvt { h, s, v, t } = v else {
        return Colour::Direct(ColourValue::WHITE);
    };
    if s <= f32::EPSILON {
        return Colour::Direct(if v < 0.5 {
            ColourValue::WHITE
        } else {
            ColourValue::BLACK
        });
    }
    Colour::Direct(ColourValue::Hsvt { h, s: 0.0, v, t })
}

/// The far value a flat transparency's new gradient gets: fully clear, in
/// its mode (mix when it had none).
#[must_use]
pub fn clear_end(t: &Transparency) -> Transparency {
    Transparency {
        level: 255,
        mode: if t.mode == TranspMode::None {
            TranspMode::Mix
        } else {
            t.mode
        },
    }
}

/// Changes the shape of an object's fill, keeping its values (T8.2.5).
#[derive(Clone, Debug, PartialEq)]
pub struct MutateFill {
    /// The object.
    pub node: NodeId,
    /// Interior or outline.
    pub slot: PaintSlot,
    /// Colour or transparency.
    pub channel: FillChannel,
    /// The new shape.
    pub to: FillShape,
}

impl Command for MutateFill {
    fn label(&self) -> &'static str {
        match self.channel {
            FillChannel::Colour => "Fill Type",
            FillChannel::Transparency => "Transparency Shape",
        }
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let doc = tx.doc();
        let bounds = doc.tree.bounds(self.node).get().unwrap_or_else(|| {
            crate::bounds::compute_bounds_with(&doc.tree, self.node, xarast_geom::Mp::ZERO)
        });
        let value = match fill_in_force(doc, self.node, self.slot, self.channel) {
            FillValue::Colour(g) => {
                let end = match &g {
                    FillGeometry::Flat { value } => desaturated(doc, value),
                    _ => Colour::Direct(ColourValue::WHITE),
                };
                FillValue::Colour(
                    mutate_fill(&g, self.to, bounds, end)
                        .map_err(EditError::FillEdit)?
                        .0,
                )
            }
            FillValue::Transparency(g) => {
                let end = match &g {
                    FillGeometry::Flat { value } => clear_end(value),
                    _ => Transparency::mix(255),
                };
                FillValue::Transparency(
                    mutate_fill(&g, self.to, bounds, end)
                        .map_err(EditError::FillEdit)?
                        .0,
                )
            }
        };
        set_own_attr(tx, self.node, value.into_attr(self.slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::RampStop;

    fn t(level: u8) -> Transparency {
        Transparency::mix(level)
    }

    fn bounds() -> Rect {
        Rect::raw(0, 0, 100_000, 50_000)
    }

    fn linear() -> FillGeometry<Transparency> {
        let mut ramp = Ramp::new();
        ramp.insert(RampStop {
            pos: 0.5,
            value: t(100),
        });
        FillGeometry::Linear {
            start: Point::raw(0, 0),
            end: Point::raw(10_000, 0),
            persp: None,
            from: t(0),
            to: t(255),
            ramp,
        }
    }

    #[test]
    fn every_pair_of_shapes_mutates_and_keeps_its_end_values() {
        for a in FillShape::ALL {
            let (src, _) = mutate_fill(&linear(), a, bounds(), t(255)).unwrap();
            assert_eq!(FillShape::of(&src), Some(a), "{a:?}");
            for b in FillShape::ALL {
                let (out, loss) = mutate_fill(&src, b, bounds(), t(200)).unwrap();
                assert_eq!(FillShape::of(&out), Some(b), "{a:?} -> {b:?}");
                let first = crate::fill_edit::stop_value(&out, crate::StopTarget::From)
                    .or_else(|| crate::fill_edit::stop_value(&out, crate::StopTarget::Corner(0)));
                assert_eq!(first, Some(t(0)), "{a:?} -> {b:?}");
                if matches!(b, FillShape::ThreeColour | FillShape::FourColour)
                    && a == FillShape::Linear
                {
                    assert!(loss.ramp_dropped);
                }
            }
        }
    }

    #[test]
    fn linear_to_radial_maps_start_to_centre_and_end_to_major() {
        let (out, loss) = mutate_fill(&linear(), FillShape::Elliptical, bounds(), t(0)).unwrap();
        let FillGeometry::Radial {
            centre,
            major,
            minor,
            ramp,
            ..
        } = out
        else {
            panic!("not radial")
        };
        assert_eq!(centre, Point::raw(0, 0));
        assert_eq!(major, Point::raw(10_000, 0));
        assert_eq!(minor, Point::raw(0, 10_000));
        assert_eq!(ramp.stops().len(), 1, "the ramp is kept");
        assert_eq!(loss, MutationLoss::default());
    }

    #[test]
    fn a_flat_fill_gets_the_bounding_box() {
        let flat = FillGeometry::Flat { value: t(30) };
        let (out, _) = mutate_fill(&flat, FillShape::Linear, bounds(), t(255)).unwrap();
        assert!(matches!(
            out,
            FillGeometry::Linear { start, end, .. }
                if start == Point::raw(0, 0) && end == Point::raw(100_000, 50_000)
        ));
        let (out, _) = mutate_fill(&flat, FillShape::Circular, bounds(), t(255)).unwrap();
        assert!(matches!(
            out,
            FillGeometry::Radial { centre, major, .. }
                if centre == Point::raw(50_000, 25_000) && major == Point::raw(75_000, 25_000)
        ));
    }
}
