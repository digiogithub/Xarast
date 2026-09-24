//! The colour editor's picking widgets (phase 8, T8.6.2, T8.6.3): a 2D
//! field over two components with a slider over a third, and the numeric
//! entry of every component with its range and unit.
//!
//! | Model | Field (x, y) | Slider |
//! |---|---|---|
//! | HSV | saturation, value | hue (drawn at full saturation and value) |
//! | RGB, CIE | the two channels not on the slider | the chosen channel |
//! | CMYK | the two inks of C, M, Y not on the slider | the chosen ink; key is numeric only |
//! | grey | — | grey |
//!
//! # Painted as meshes, not textures
//!
//! The field is one mesh of vertex-coloured quads. Over an RGB plane the
//! colour is linear in both axes and over an HSV saturation × value square
//! it is bilinear (`V · (1 − S · k)` per channel), so the rasteriser's own
//! interpolation of the vertex colours is **exact** at any grid size; only
//! CMYK's clip at `C + K = 1` bends, and a 16 × 16 grid follows it to
//! within the width of a pixel. No texture is uploaded, regenerated or
//! cached, which is simpler than the texture-per-fixed-axis mitigation the
//! phase plan proposed and costs 289 vertices a frame.
//!
//! # Interaction
//!
//! Pointer down (and every move while it is held) previews; release
//! commits; `Esc` while held cancels and ignores the rest of that press.
//! A focused field or slider moves with the arrow keys by 1 % (10 % with
//! `Shift`), each press committed on its own.

use egui::{Color32, Pos2, Rect, Vec2};
use xarast_color::{ColourModel, ColourValue};

/// How one component is shown and typed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComponentSpec {
    /// Its index in the model's four components.
    pub index: usize,
    /// Its name, which is also its accessible name.
    pub name: &'static str,
    /// The displayed value of 1.0.
    pub max: f64,
    /// The unit after the number.
    pub suffix: &'static str,
}

impl ComponentSpec {
    /// The number shown for a component value.
    #[must_use]
    pub fn to_display(&self, v: f32) -> f64 {
        (f64::from(v) * self.max).clamp(0.0, self.max)
    }

    /// The component value of a typed number, clamped to the range.
    #[must_use]
    pub fn from_display(&self, d: f64) -> f32 {
        if d.is_nan() {
            return 0.0;
        }
        ((d / self.max).clamp(0.0, 1.0)) as f32
    }
}

const fn spec(index: usize, name: &'static str, max: f64, suffix: &'static str) -> ComponentSpec {
    ComponentSpec {
        index,
        name,
        max,
        suffix,
    }
}

/// The components a model shows, in order, with their ranges and units:
/// RGB channels in 0–255 like every other editor, the hue in degrees,
/// everything else in per cent.
#[must_use]
pub fn component_specs(model: ColourModel) -> Vec<ComponentSpec> {
    let t = |i| spec(i, "Transparency", 100.0, " %");
    match model {
        ColourModel::Hsvt => vec![
            spec(0, "Hue", 360.0, "\u{b0}"),
            spec(1, "Saturation", 100.0, " %"),
            spec(2, "Value", 100.0, " %"),
            t(3),
        ],
        ColourModel::Cmyk => vec![
            spec(0, "Cyan", 100.0, " %"),
            spec(1, "Magenta", 100.0, " %"),
            spec(2, "Yellow", 100.0, " %"),
            spec(3, "Key", 100.0, " %"),
        ],
        ColourModel::Greyt => vec![spec(0, "Grey", 100.0, " %"), t(1)],
        ColourModel::Ciet => vec![
            spec(0, "X", 100.0, " %"),
            spec(1, "Y", 100.0, " %"),
            spec(2, "Z", 100.0, " %"),
            t(3),
        ],
        ColourModel::Rgbt | ColourModel::WebRgbt | ColourModel::Indexed => vec![
            spec(0, "Red", 255.0, ""),
            spec(1, "Green", 255.0, ""),
            spec(2, "Blue", 255.0, ""),
            t(3),
        ],
    }
}

/// Which components the field and its slider drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldAxes {
    /// The component along the field's width, growing to the right.
    pub x: usize,
    /// The component along the field's height, growing upwards.
    pub y: usize,
    /// The component on the slider.
    pub slider: usize,
}

/// The components that can go on the slider, for the "slider" chooser.
/// Empty when the model has no choice (HSV: always the hue).
#[must_use]
pub fn slider_choices(model: ColourModel) -> &'static [usize] {
    match model {
        ColourModel::Hsvt | ColourModel::Greyt => &[],
        _ => &[0, 1, 2],
    }
}

/// The field's axes with component `slider` on the slider, or `None` for
/// a model with no field (grey: the slider alone).
#[must_use]
pub fn field_axes(model: ColourModel, slider: usize) -> Option<FieldAxes> {
    match model {
        ColourModel::Greyt => None,
        ColourModel::Hsvt => Some(FieldAxes {
            x: 1,
            y: 2,
            slider: 0,
        }),
        _ => {
            let slider = slider.min(2);
            let mut rest = (0..3).filter(|i| *i != slider);
            let x = rest.next().unwrap_or(0);
            let y = rest.next().unwrap_or(1);
            Some(FieldAxes { x, y, slider })
        }
    }
}

/// An opaque swatch colour for components in `model`: the field shows the
/// colour, not its transparency.
#[must_use]
pub fn opaque(model: ColourModel, comps: [f32; 4]) -> Color32 {
    let rgba = ColourValue::from_components(model, comps).to_rgba8();
    Color32::from_rgb(rgba.r, rgba.g, rgba.b)
}

/// The colour at field position `(fx, fy)`, each `0..=1`, `fy` upwards.
#[must_use]
pub fn field_colour(
    model: ColourModel,
    comps: [f32; 4],
    axes: FieldAxes,
    fx: f32,
    fy: f32,
) -> Color32 {
    let mut c = comps;
    c[axes.x] = fx;
    c[axes.y] = fy;
    opaque(model, c)
}

/// The colour at slider position `t`, `0..=1`. The hue strip is drawn at
/// full saturation and value, so it always shows the hues; any other
/// strip shows the colour it would give.
#[must_use]
pub fn slider_colour(model: ColourModel, comps: [f32; 4], slider: usize, t: f32) -> Color32 {
    let mut c = comps;
    c[slider] = t;
    if model == ColourModel::Hsvt && slider == 0 {
        c[1] = 1.0;
        c[2] = 1.0;
    }
    opaque(model, c)
}

/// The field fraction `(fx, fy)` of a point in `rect`, clamped, `fy`
/// upwards.
#[must_use]
pub fn field_fraction(rect: Rect, p: Pos2) -> (f32, f32) {
    let fx = ((p.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    let fy = ((rect.bottom() - p.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
    (fx, fy)
}

/// The point of `rect` at field fraction `(fx, fy)`.
#[must_use]
pub fn field_point(rect: Rect, fx: f32, fy: f32) -> Pos2 {
    Pos2::new(
        rect.left() + fx * rect.width(),
        rect.bottom() - fy * rect.height(),
    )
}

/// Vertices per side of the field's mesh.
pub const FIELD_GRID: usize = 16;

/// Segments of a slider strip's mesh.
pub const STRIP_SEGMENTS: usize = 36;

/// The field as one mesh of `FIELD_GRID²` vertex-coloured quads.
#[must_use]
pub fn field_mesh(rect: Rect, colour: impl Fn(f32, f32) -> Color32) -> egui::Mesh {
    let n = FIELD_GRID;
    let mut mesh = egui::Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let (fx, fy) = (i as f32 / n as f32, j as f32 / n as f32);
            mesh.colored_vertex(field_point(rect, fx, fy), colour(fx, fy));
        }
    }
    let w = u32::try_from(n + 1).unwrap_or(u32::MAX);
    for j in 0..w - 1 {
        for i in 0..w - 1 {
            let a = j * w + i;
            mesh.add_triangle(a, a + 1, a + w);
            mesh.add_triangle(a + 1, a + w + 1, a + w);
        }
    }
    mesh
}

/// A strip as a mesh: `t` grows along the strip (upwards when vertical).
#[must_use]
pub fn strip_mesh(rect: Rect, vertical: bool, colour: impl Fn(f32) -> Color32) -> egui::Mesh {
    let n = STRIP_SEGMENTS;
    let mut mesh = egui::Mesh::default();
    for k in 0..=n {
        let t = k as f32 / n as f32;
        let c = colour(t);
        let (a, b) = if vertical {
            let y = rect.bottom() - t * rect.height();
            (Pos2::new(rect.left(), y), Pos2::new(rect.right(), y))
        } else {
            let x = rect.left() + t * rect.width();
            (Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom()))
        };
        mesh.colored_vertex(a, c);
        mesh.colored_vertex(b, c);
    }
    for k in 0..u32::try_from(n).unwrap_or(0) {
        let a = 2 * k;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    mesh
}

/// What a picking widget did this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick<T> {
    /// Nothing.
    None,
    /// A live value while the pointer is held.
    Live(T),
    /// A value made in one go (an arrow key).
    Set(T),
    /// The press ended: keep what the live values did.
    Commit,
    /// `Esc` during the press: take it back.
    Cancel,
}

/// The press-tracking state a picking widget keeps between frames (the
/// only state a panel may keep: interaction, not document).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PressState {
    held: Option<egui::Id>,
    cancelled: bool,
}

impl PressState {
    /// Whether a press on `id` is in flight.
    #[must_use]
    pub fn holding(&self, id: egui::Id) -> bool {
        self.held == Some(id)
    }

    /// Turns a widget's response into a [`Pick`] of the pointer position.
    pub fn track(&mut self, ui: &egui::Ui, response: &egui::Response) -> Pick<Pos2> {
        let id = response.id;
        let down = response.is_pointer_button_down_on();
        if down {
            if self.held != Some(id) {
                self.held = Some(id);
                self.cancelled = false;
            }
            if self.cancelled {
                return Pick::None;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.cancelled = true;
                return Pick::Cancel;
            }
            return match response.interact_pointer_pos() {
                Some(p) => Pick::Live(p),
                None => Pick::None,
            };
        }
        if self.held == Some(id) {
            self.held = None;
            if std::mem::take(&mut self.cancelled) {
                return Pick::None;
            }
            // egui drops the press itself when `Esc` is pressed, so the
            // widget stops being "down" in that very frame: that is a
            // cancel, not a release (XARA-T-0305, measured).
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                return Pick::Cancel;
            }
            return Pick::Commit;
        }
        Pick::None
    }
}

/// The arrow-key step of a focused picker: `(dx, dy)` in component units,
/// 1 % a press, 10 % with `Shift`.
#[must_use]
pub fn arrow_step(ui: &egui::Ui, response: &egui::Response) -> Option<Vec2> {
    if !response.has_focus() {
        return None;
    }
    // Keep the arrows for this widget rather than for moving the focus.
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            response.id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
    ui.input(|i| {
        let step = if i.modifiers.shift { 0.1 } else { 0.01 };
        let mut d = Vec2::ZERO;
        if i.key_pressed(egui::Key::ArrowLeft) {
            d.x -= step;
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            d.x += step;
        }
        if i.key_pressed(egui::Key::ArrowDown) {
            d.y -= step;
        }
        if i.key_pressed(egui::Key::ArrowUp) {
            d.y += step;
        }
        (d != Vec2::ZERO).then_some(d)
    })
}

/// Paints the marker ring at `p`, dark inside light so it shows on any
/// colour.
pub fn marker(painter: &egui::Painter, p: Pos2) {
    painter.circle_stroke(p, 5.0, egui::Stroke::new(2.5_f32, Color32::BLACK));
    painter.circle_stroke(p, 5.0, egui::Stroke::new(1.0_f32, Color32::WHITE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_has_named_ranged_components() {
        for model in [
            ColourModel::Rgbt,
            ColourModel::Hsvt,
            ColourModel::Greyt,
            ColourModel::Cmyk,
            ColourModel::Ciet,
        ] {
            let specs = component_specs(model);
            assert!(specs.len() >= 2, "{model:?}");
            for s in &specs {
                assert!(!s.name.is_empty());
                assert!(s.max > 0.0);
                assert!(s.index < 4);
            }
        }
        let rgb = component_specs(ColourModel::Rgbt);
        assert_eq!(rgb[0].to_display(1.0), 255.0);
        let hsv = component_specs(ColourModel::Hsvt);
        assert_eq!(hsv[0].to_display(0.5), 180.0);
        assert_eq!(hsv[0].suffix, "\u{b0}");
        let grey = component_specs(ColourModel::Greyt);
        assert_eq!(grey[1].index, 1, "a grey's transparency is its second");
    }

    #[test]
    fn typed_numbers_are_clamped_to_the_range() {
        let red = component_specs(ColourModel::Rgbt)[0];
        assert_eq!(red.from_display(510.0), 1.0);
        assert_eq!(red.from_display(-3.0), 0.0);
        assert_eq!(red.from_display(f64::NAN), 0.0);
        assert!((red.from_display(51.0) - 0.2).abs() < 1e-6);
        for v in [0.0_f32, 0.25, 0.5, 1.0] {
            assert!((red.from_display(red.to_display(v)) - v).abs() < 1e-6);
        }
    }

    #[test]
    fn the_field_takes_the_components_the_slider_does_not() {
        assert_eq!(
            field_axes(ColourModel::Hsvt, 2),
            Some(FieldAxes {
                x: 1,
                y: 2,
                slider: 0
            })
        );
        assert_eq!(
            field_axes(ColourModel::Rgbt, 1),
            Some(FieldAxes {
                x: 0,
                y: 2,
                slider: 1
            })
        );
        assert_eq!(
            field_axes(ColourModel::Cmyk, 9),
            Some(FieldAxes {
                x: 0,
                y: 1,
                slider: 2
            }),
            "key is never on the field"
        );
        assert_eq!(field_axes(ColourModel::Greyt, 0), None);
    }

    #[test]
    fn the_field_shows_the_colour_each_point_would_give() {
        let axes = field_axes(ColourModel::Rgbt, 0).unwrap();
        let c = [0.5, 0.0, 0.0, 0.0];
        assert_eq!(
            field_colour(ColourModel::Rgbt, c, axes, 1.0, 0.0),
            Color32::from_rgb(128, 255, 0)
        );
        // HSV: top right is the pure hue, the bottom edge is black, the
        // left edge grey.
        let hsv = field_axes(ColourModel::Hsvt, 0).unwrap();
        let h = [0.0, 0.3, 0.3, 0.0];
        assert_eq!(
            field_colour(ColourModel::Hsvt, h, hsv, 1.0, 1.0),
            Color32::from_rgb(255, 0, 0)
        );
        assert_eq!(
            field_colour(ColourModel::Hsvt, h, hsv, 0.7, 0.0),
            Color32::BLACK
        );
        assert_eq!(
            field_colour(ColourModel::Hsvt, h, hsv, 0.0, 1.0),
            Color32::WHITE
        );
        // The hue strip ignores the current saturation and value.
        assert_eq!(
            slider_colour(ColourModel::Hsvt, [0.0, 0.0, 0.0, 0.0], 0, 1.0 / 3.0),
            Color32::from_rgb(0, 255, 0)
        );
        // Transparency never reaches the field.
        assert_eq!(
            field_colour(ColourModel::Rgbt, [1.0, 1.0, 1.0, 1.0], axes, 1.0, 1.0).a(),
            255
        );
    }

    /// The claim that justifies meshes over textures: over an HSV square,
    /// interpolating the vertex colours is exact.
    #[test]
    fn bilinear_vertex_colours_are_exact_over_rgb_and_hsv() {
        let hsv = field_axes(ColourModel::Hsvt, 0).unwrap();
        let comps = [0.58, 0.0, 0.0, 0.0];
        let at = |fx: f32, fy: f32| {
            let mut c = comps;
            c[hsv.x] = fx;
            c[hsv.y] = fy;
            ColourValue::from_components(ColourModel::Hsvt, c)
                .to_rgbt()
                .components()
        };
        let step = 1.0 / FIELD_GRID as f32;
        for (fx, fy) in [(0.03_f32, 0.91_f32), (0.5, 0.5), (0.77, 0.12)] {
            let (i, j) = ((fx / step).floor(), (fy / step).floor());
            let (x0, y0) = (i * step, j * step);
            let (u, v) = ((fx - x0) / step, (fy - y0) / step);
            let (a, b, c, d) = (
                at(x0, y0),
                at(x0 + step, y0),
                at(x0, y0 + step),
                at(x0 + step, y0 + step),
            );
            let exact = at(fx, fy);
            for k in 0..3 {
                let lerp =
                    (1.0 - v) * ((1.0 - u) * a[k] + u * b[k]) + v * ((1.0 - u) * c[k] + u * d[k]);
                assert!(
                    (lerp - exact[k]).abs() < 1e-4,
                    "channel {k} at ({fx}, {fy}): {lerp} vs {}",
                    exact[k]
                );
            }
        }
    }

    #[test]
    fn fractions_and_points_are_inverse_and_y_grows_upwards() {
        let r = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(100.0, 50.0));
        assert_eq!(field_fraction(r, r.left_bottom()), (0.0, 0.0));
        assert_eq!(field_fraction(r, r.right_top()), (1.0, 1.0));
        assert_eq!(field_fraction(r, Pos2::new(-50.0, 999.0)), (0.0, 0.0));
        let p = field_point(r, 0.25, 0.75);
        let (fx, fy) = field_fraction(r, p);
        assert!((fx - 0.25).abs() < 1e-6 && (fy - 0.75).abs() < 1e-6);
    }

    #[test]
    fn meshes_have_the_expected_size() {
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0));
        let m = field_mesh(r, |_, _| Color32::RED);
        assert_eq!(m.vertices.len(), (FIELD_GRID + 1) * (FIELD_GRID + 1));
        assert_eq!(m.indices.len(), FIELD_GRID * FIELD_GRID * 6);
        let s = strip_mesh(r, true, |_| Color32::RED);
        assert_eq!(s.vertices.len(), 2 * (STRIP_SEGMENTS + 1));
        assert_eq!(s.indices.len(), STRIP_SEGMENTS * 6);
        assert!(m.is_valid() && s.is_valid());
    }
}
