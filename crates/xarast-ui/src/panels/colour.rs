//! The colour panel: the colour editor and the on-screen colour line.
//!
//! The **colour editor** (phase 8, W8.6), as `research/04 §1.5` describes
//! it: what it edits — the selection's fill or line (the fill tool's
//! selected stop, when it has one), or a named colour — a 2D field with a
//! slider, numeric entry of every component, the colour's derivation when
//! it is a named colour, and the two explicit actions "Redefine" and "Apply
//! to the selection". It draws
//! [`xarast_app::colour_editor::ColourEditorView`] and answers with
//! [`UiCommand::ColourEditor`]; the model, the live/committed split and the
//! undo steps are `xarast-app`'s.
//!
//! The colour line that used to sit under the editor is now the colour bar
//! below the canvas ([`crate::colour_bar`], W8.7).

use xarast_app::colour_editor::{
    ColourChange, ColourEditorOp, ColourEditorView, ColourTarget, Derivation, NamedColour,
    PaintSlot,
};
use xarast_color::{ColourId, ColourModel, ColourValue};

use crate::a11y;
use crate::colour_field::{
    self, ComponentSpec, Pick, PressState, component_specs, field_axes, field_colour,
    field_fraction, field_point, slider_choices, slider_colour,
};
use crate::model::{PaletteEntry, UiCommand};
use crate::panel::{Panel, PanelCtx, PanelId};
use crate::theme::SWATCH_SIZE;

/// The colour panel's identifier.
pub const ID: PanelId = PanelId("colour");

/// The models the editor offers, in the order of its tabs.
pub const MODELS: [ColourModel; 4] = [
    ColourModel::Rgbt,
    ColourModel::Hsvt,
    ColourModel::Greyt,
    ColourModel::Cmyk,
];

/// The side of the 2D field, in points.
const FIELD_SIDE: f32 = 128.0;
/// The width of the slider strip, in points.
const STRIP_WIDTH: f32 = 18.0;

/// The colour panel. Everything it keeps is interaction state: which
/// component is on the slider, the press in flight, a name being typed.
#[derive(Debug, Default)]
pub struct ColourPanel {
    slider: usize,
    press: PressState,
    cancelled_drag: Option<egui::Id>,
    name_edit: Option<(ColourId, String)>,
}

impl ColourPanel {
    /// A fresh panel.
    pub fn new() -> ColourPanel {
        ColourPanel::default()
    }

    /// Which component the RGB or CMYK slider drives.
    pub fn slider_component(&self) -> usize {
        self.slider
    }
}

/// The component names of a colour model, in order.
///
/// Four names always, with the fourth being the transparency `xarast-color`
/// carries in every model.
pub fn component_names(model: ColourModel) -> [&'static str; 4] {
    let mut names = [""; 4];
    for s in component_specs(model) {
        names[s.index] = s.name;
    }
    names
}

/// How many components a model actually shows.
pub fn component_count(model: ColourModel) -> usize {
    component_specs(model).len()
}

/// The label of a model's tab.
pub fn model_label(model: ColourModel) -> &'static str {
    match model {
        ColourModel::Rgbt => "RGB",
        ColourModel::Cmyk => "CMYK",
        ColourModel::Hsvt => "HSV",
        ColourModel::Greyt => "Grey",
        ColourModel::Ciet => "CIE",
        ColourModel::Indexed => "Indexed",
        ColourModel::WebRgbt => "Web RGB",
    }
}

/// The swatch colour for a palette entry, as egui wants it.
///
/// "No colour" is drawn as a hollow swatch with a diagonal, never as white:
/// the distinction matters and a user must be able to see it.
pub fn swatch_colour(entry: &PaletteEntry) -> Option<egui::Color32> {
    entry.colour.map(colour32)
}

fn colour32(c: ColourValue) -> egui::Color32 {
    let rgba = c.to_rgba8();
    egui::Color32::from_rgba_unmultiplied(rgba.r, rgba.g, rgba.b, rgba.a)
}

/// The hexadecimal form shown next to the preview.
pub fn hex_of(colour: ColourValue) -> String {
    let c = colour.to_rgba8();
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

/// The name a new colour gets: the first free "Colour n".
pub fn fresh_name(named: &[NamedColour]) -> String {
    (1..)
        .map(|n| format!("Colour {n}"))
        .find(|n| named.iter().all(|c| &c.name != n))
        .unwrap_or_else(|| "Colour".to_owned())
}

/// What the target chooser calls a target.
pub fn target_label(target: ColourTarget, named: &[NamedColour]) -> String {
    match target {
        ColourTarget::Selection(PaintSlot::Fill) => "Selection fill".to_owned(),
        ColourTarget::Selection(PaintSlot::Stroke) => "Selection line".to_owned(),
        ColourTarget::Entry(id) => named
            .iter()
            .find(|n| n.id == id)
            .map_or_else(|| "Named colour".to_owned(), |n| n.name.clone()),
    }
}

/// How a value widget was used this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    None,
    Live,
    Set,
    Commit,
    Cancel,
}

fn emit(ctx: &mut PanelCtx<'_>, op: ColourEditorOp) {
    ctx.out.push(UiCommand::ColourEditor(op));
}

fn emit_change(ctx: &mut PanelCtx<'_>, how: Use, change: ColourChange) {
    match how {
        Use::Live => emit(ctx, ColourEditorOp::Preview(change)),
        Use::Set => emit(ctx, ColourEditorOp::Set(change)),
        Use::Commit => emit(ctx, ColourEditorOp::Commit),
        Use::Cancel => emit(ctx, ColourEditorOp::Cancel),
        Use::None => {}
    }
}

impl Panel for ColourPanel {
    fn id(&self) -> PanelId {
        ID
    }

    fn title(&self) -> &str {
        "Colour"
    }

    fn min_size(&self) -> egui::Vec2 {
        egui::vec2(240.0, 200.0)
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        let view = ctx.model.colour_editor.clone();
        match &view {
            Some(v) => {
                egui::ScrollArea::vertical()
                    .id_salt("xarast_colour_editor")
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.editor(ui, v, ctx));
            }
            None => {
                ui.label("Open a document to edit its colours.");
            }
        }
    }
}

impl ColourPanel {
    /// Classifies a value widget's use, remembering an `Esc` during its
    /// drag so the rest of that drag is ignored.
    fn classify(&mut self, ui: &egui::Ui, r: &egui::Response) -> Use {
        if r.dragged() {
            if self.cancelled_drag == Some(r.id) {
                return Use::None;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.cancelled_drag = Some(r.id);
                return Use::Cancel;
            }
            return if r.changed() { Use::Live } else { Use::None };
        }
        if r.drag_stopped() {
            if self.cancelled_drag == Some(r.id) {
                self.cancelled_drag = None;
                return Use::None;
            }
            return Use::Commit;
        }
        if r.changed() { Use::Set } else { Use::None }
    }

    fn editor(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, ctx: &mut PanelCtx<'_>) {
        self.target_row(ui, v, ctx);
        ui.label(egui::RichText::new(&v.title).color(ctx.tokens.text));
        self.model_row(ui, v, ctx);
        ui.horizontal(|ui| self.field(ui, v, ctx));
        self.numbers(ui, v, ctx);
        preview_row(ui, v);
        self.links(ui, v, ctx);
        if let (ColourTarget::Entry(id), Some((name, d))) = (v.target, &v.entry) {
            ui.separator();
            self.entry_editor(ui, v, id, name, *d, ctx);
        }
    }

    fn target_row(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, ctx: &mut PanelCtx<'_>) {
        let shown = target_label(v.target, &v.named);
        let mut picked = None;
        ui.horizontal(|ui| {
            let r = egui::ComboBox::from_id_salt("xarast_colour_target")
                .selected_text(&shown)
                .show_ui(ui, |ui| {
                    for slot in [PaintSlot::Fill, PaintSlot::Stroke] {
                        let t = ColourTarget::Selection(slot);
                        if ui
                            .selectable_label(v.target == t, target_label(t, &v.named))
                            .clicked()
                        {
                            picked = Some(t);
                        }
                    }
                    for n in &v.named {
                        let t = ColourTarget::Entry(n.id);
                        if ui.selectable_label(v.target == t, &n.name).clicked() {
                            picked = Some(t);
                        }
                    }
                });
            a11y::set_label(ui.ctx(), r.response.id, format!("Edit: {shown}"));
            if ui
                .button("New colour")
                .on_hover_text("Make a named colour holding this colour, and edit it")
                .clicked()
            {
                emit(ctx, ColourEditorOp::NewNamed(fresh_name(&v.named)));
            }
        });
        if let Some(t) = picked {
            emit(ctx, ColourEditorOp::SetTarget(t));
        }
    }

    fn model_row(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, ctx: &mut PanelCtx<'_>) {
        ui.horizontal(|ui| {
            for model in MODELS {
                let r = ui.add_enabled(
                    v.model_editable || v.model == model,
                    egui::Button::selectable(v.model == model, model_label(model)),
                );
                if r.clicked() && v.model != model {
                    emit(ctx, ColourEditorOp::SetModel(model));
                }
            }
            let choices = slider_choices(v.model);
            if !choices.is_empty() {
                ui.separator();
                let names = component_names(v.model);
                let shown = names[self.slider.min(2)];
                let r = egui::ComboBox::from_id_salt("xarast_colour_slider")
                    .selected_text(shown)
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        for &c in choices {
                            ui.selectable_value(&mut self.slider, c, names[c]);
                        }
                    });
                a11y::set_label(ui.ctx(), r.response.id, format!("Slider: {shown}"));
            }
        });
    }

    /// The 2D field and its slider strip (T8.6.2).
    fn field(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, ctx: &mut PanelCtx<'_>) {
        let names = component_names(v.model);
        let comps = v.components;
        let axes = field_axes(v.model, self.slider);
        if let Some(axes) = axes {
            let enabled = v.editable[axes.x] || v.editable[axes.y];
            let (rect, r) = ui.allocate_exact_size(
                egui::vec2(FIELD_SIDE, FIELD_SIDE),
                if enabled {
                    egui::Sense::click_and_drag()
                } else {
                    egui::Sense::hover()
                },
            );
            let label = format!("{} (across) and {} (up)", names[axes.x], names[axes.y]);
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, enabled, label.clone())
            });
            let painter = ui.painter_at(rect);
            painter.add(egui::Shape::mesh(colour_field::field_mesh(
                rect,
                |fx, fy| field_colour(v.model, comps, axes, fx, fy),
            )));
            colour_field::marker(&painter, field_point(rect, comps[axes.x], comps[axes.y]));
            if r.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0_f32, ctx.tokens.focus),
                    egui::StrokeKind::Outside,
                );
            }
            if enabled {
                let set = |fx: f32, fy: f32| {
                    let mut c = comps;
                    c[axes.x] = fx;
                    c[axes.y] = fy;
                    ColourChange::Components(c)
                };
                self.pick(ui, &r, ctx, |p| {
                    let (fx, fy) = field_fraction(rect, p);
                    set(fx, fy)
                });
                if let Some(d) = colour_field::arrow_step(ui, &r) {
                    let fx = (comps[axes.x] + d.x).clamp(0.0, 1.0);
                    let fy = (comps[axes.y] + d.y).clamp(0.0, 1.0);
                    emit(ctx, ColourEditorOp::Set(set(fx, fy)));
                }
            }
        }
        let slider = axes.map_or(0, |a| a.slider);
        self.strip(ui, v, slider, ctx);
    }

    fn strip(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, k: usize, ctx: &mut PanelCtx<'_>) {
        let comps = v.components;
        let vertical = v.model != ColourModel::Greyt;
        let size = if vertical {
            egui::vec2(STRIP_WIDTH, FIELD_SIDE)
        } else {
            egui::vec2(FIELD_SIDE + STRIP_WIDTH, STRIP_WIDTH)
        };
        let enabled = v.editable[k];
        let (rect, r) = ui.allocate_exact_size(
            size,
            if enabled {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::hover()
            },
        );
        let name = component_names(v.model)[k];
        let value = f64::from(comps[k]);
        r.widget_info(|| {
            let mut info = egui::WidgetInfo::slider(enabled, value, name);
            info.label = Some(format!("{name} slider"));
            info
        });
        let painter = ui.painter_at(rect);
        painter.add(egui::Shape::mesh(colour_field::strip_mesh(
            rect,
            vertical,
            |t| slider_colour(v.model, comps, k, t),
        )));
        let at = if vertical {
            egui::pos2(rect.center().x, rect.bottom() - comps[k] * rect.height())
        } else {
            egui::pos2(rect.left() + comps[k] * rect.width(), rect.center().y)
        };
        colour_field::marker(&painter, at);
        if !enabled {
            return;
        }
        let set = |t: f32| {
            let mut c = comps;
            c[k] = t.clamp(0.0, 1.0);
            ColourChange::Components(c)
        };
        self.pick(ui, &r, ctx, |p| {
            let (fx, fy) = field_fraction(rect, p);
            set(if vertical { fy } else { fx })
        });
        if let Some(d) = colour_field::arrow_step(ui, &r) {
            emit(ctx, ColourEditorOp::Set(set(comps[k] + d.x + d.y)));
        }
    }

    fn pick(
        &mut self,
        ui: &egui::Ui,
        r: &egui::Response,
        ctx: &mut PanelCtx<'_>,
        at: impl Fn(egui::Pos2) -> ColourChange,
    ) {
        match self.press.track(ui, r) {
            Pick::Live(p) => emit(ctx, ColourEditorOp::Preview(at(p))),
            Pick::Set(p) => emit(ctx, ColourEditorOp::Set(at(p))),
            Pick::Commit => emit(ctx, ColourEditorOp::Commit),
            Pick::Cancel => emit(ctx, ColourEditorOp::Cancel),
            Pick::None => {}
        }
    }

    /// Numeric entry of every component (T8.6.3): typed, dragged or
    /// stepped with the arrow keys, clamped to its range.
    fn numbers(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, ctx: &mut PanelCtx<'_>) {
        egui::Grid::new("xarast_colour_numbers")
            .num_columns(2)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                for spec in component_specs(v.model) {
                    self.number(ui, v, spec, ctx);
                    ui.end_row();
                }
            });
    }

    fn number(
        &mut self,
        ui: &mut egui::Ui,
        v: &ColourEditorView,
        spec: ComponentSpec,
        ctx: &mut PanelCtx<'_>,
    ) {
        ui.label(spec.name);
        let mut shown = spec.to_display(v.components[spec.index]);
        let r = ui.add_enabled(
            v.editable[spec.index],
            egui::DragValue::new(&mut shown)
                .range(0.0..=spec.max)
                .suffix(spec.suffix)
                .max_decimals(if spec.max > 200.0 { 0 } else { 1 })
                .speed(spec.max / 200.0),
        );
        a11y::set_label(ui.ctx(), r.id, spec.name);
        let how = self.classify(ui, &r);
        let mut c = v.components;
        c[spec.index] = spec.from_display(shown);
        emit_change(ctx, how, ColourChange::Components(c));
    }

    /// The link notice and the two explicit actions (T8.6.5).
    fn links(&mut self, ui: &mut egui::Ui, v: &ColourEditorView, ctx: &mut PanelCtx<'_>) {
        match v.target {
            ColourTarget::Selection(_) => {
                if let Some(e) = &v.linked_entry {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "Uses \u{2018}{}\u{2019}: editing here unlinks it.",
                            e.name
                        ));
                        if ui
                            .button(format!("Redefine \u{2018}{}\u{2019}", e.name))
                            .on_hover_text("Edit the named colour: every object using it changes")
                            .clicked()
                        {
                            emit(ctx, ColourEditorOp::SetTarget(ColourTarget::Entry(e.id)));
                        }
                    });
                }
            }
            ColourTarget::Entry(id) => {
                ui.horizontal(|ui| {
                    for (slot, text) in [
                        (PaintSlot::Fill, "Apply to fill"),
                        (PaintSlot::Stroke, "Apply to line"),
                    ] {
                        if ui
                            .button(text)
                            .on_hover_text("Put this named colour on the selection")
                            .clicked()
                        {
                            emit(ctx, ColourEditorOp::ApplyEntry { id, slot });
                        }
                    }
                });
            }
        }
    }

    /// The derivation editor (T8.6.4): name, kind, parent and amounts.
    fn entry_editor(
        &mut self,
        ui: &mut egui::Ui,
        v: &ColourEditorView,
        id: ColourId,
        name: &str,
        d: Derivation,
        ctx: &mut PanelCtx<'_>,
    ) {
        // Name: committed on Enter or when the field loses the focus.
        ui.horizontal(|ui| {
            ui.label("Name");
            let buf = match &mut self.name_edit {
                Some((eid, b)) if *eid == id => b,
                slot => {
                    *slot = Some((id, name.to_owned()));
                    &mut slot.as_mut().expect("just set").1
                }
            };
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(140.0));
            a11y::set_label(ui.ctx(), r.id, "Colour name");
            if r.lost_focus() && buf.trim() != name {
                emit(ctx, ColourEditorOp::Rename(buf.clone()));
            } else if !r.has_focus() && buf.as_str() != name {
                *buf = name.to_owned();
            }
        });

        let parents: Vec<&NamedColour> = v.named.iter().filter(|n| n.can_parent).collect();
        let default_parent = d.parent().or_else(|| parents.first().map(|p| p.id));
        let kinds: Vec<Derivation> = {
            let mut k = vec![Derivation::Normal, Derivation::Spot];
            if let Some(parent) = default_parent {
                k.push(match d {
                    Derivation::Tint { .. } => d,
                    _ => Derivation::Tint {
                        parent,
                        factor: 0.5,
                    },
                });
                k.push(match d {
                    Derivation::Shade { .. } => d,
                    _ => Derivation::Shade {
                        parent,
                        x: 0.0,
                        y: -0.5,
                    },
                });
                k.push(match d {
                    Derivation::Linked { .. } => d,
                    _ => Derivation::Linked {
                        parent,
                        model: v.model,
                        inherit: [false; 4],
                    },
                });
            }
            k
        };
        ui.horizontal(|ui| {
            let mut picked = None;
            let r = egui::ComboBox::from_id_salt("xarast_colour_kind")
                .selected_text(d.label())
                .show_ui(ui, |ui| {
                    for k in &kinds {
                        if ui
                            .selectable_label(k.label() == d.label(), k.label())
                            .clicked()
                            && k.label() != d.label()
                        {
                            picked = Some(*k);
                        }
                    }
                });
            a11y::set_label(
                ui.ctx(),
                r.response.id,
                format!("Colour type: {}", d.label()),
            );
            if let Some(k) = picked {
                emit(ctx, ColourEditorOp::Set(ColourChange::Derivation(k)));
            }
            if let Some(parent) = d.parent() {
                let shown = v
                    .named
                    .iter()
                    .find(|n| n.id == parent)
                    .map_or("?", |n| n.name.as_str())
                    .to_owned();
                let mut picked = None;
                let r = egui::ComboBox::from_id_salt("xarast_colour_parent")
                    .selected_text(&shown)
                    .show_ui(ui, |ui| {
                        for p in &parents {
                            if ui.selectable_label(p.id == parent, &p.name).clicked() {
                                picked = Some(p.id);
                            }
                        }
                    });
                a11y::set_label(ui.ctx(), r.response.id, format!("Parent colour: {shown}"));
                if let Some(p) = picked.filter(|p| *p != parent) {
                    emit(
                        ctx,
                        ColourEditorOp::Set(ColourChange::Derivation(d.with_parent(p))),
                    );
                }
            }
        });

        match d {
            Derivation::Tint { parent, factor } => {
                let mut pct = f64::from(factor) * 100.0;
                let r = ui.add(
                    egui::Slider::new(&mut pct, 0.0..=100.0)
                        .text("Tint")
                        .suffix(" %")
                        .fixed_decimals(0),
                );
                let how = self.classify(ui, &r);
                emit_change(
                    ctx,
                    how,
                    ColourChange::Derivation(Derivation::Tint {
                        parent,
                        factor: (pct / 100.0) as f32,
                    }),
                );
            }
            Derivation::Shade { parent, x, y } => {
                let mut sx = f64::from(x) * 100.0;
                let mut sy = f64::from(y) * 100.0;
                let rx = ui.add(
                    egui::Slider::new(&mut sx, -100.0..=100.0)
                        .text("Saturation shift")
                        .suffix(" %")
                        .fixed_decimals(0),
                );
                let hx = self.classify(ui, &rx);
                let ry = ui.add(
                    egui::Slider::new(&mut sy, -100.0..=100.0)
                        .text("Brightness shift")
                        .suffix(" %")
                        .fixed_decimals(0),
                );
                let hy = self.classify(ui, &ry);
                let change = ColourChange::Derivation(Derivation::Shade {
                    parent,
                    x: (sx / 100.0) as f32,
                    y: (sy / 100.0) as f32,
                });
                emit_change(ctx, if hx == Use::None { hy } else { hx }, change);
            }
            Derivation::Linked {
                parent,
                model,
                inherit,
            } => {
                ui.label("Follow the parent's:");
                ui.horizontal_wrapped(|ui| {
                    for spec in component_specs(model) {
                        let mut on = inherit[spec.index];
                        if ui.checkbox(&mut on, spec.name).changed() {
                            let mut inherit = inherit;
                            inherit[spec.index] = on;
                            emit(
                                ctx,
                                ColourEditorOp::Set(ColourChange::Derivation(Derivation::Linked {
                                    parent,
                                    model,
                                    inherit,
                                })),
                            );
                        }
                    }
                });
            }
            Derivation::Normal | Derivation::Spot => {}
        }
    }
}

fn preview_row(ui: &mut egui::Ui, v: &ColourEditorView) {
    ui.horizontal(|ui| {
        let (rect, r) = ui.allocate_exact_size(
            egui::vec2(SWATCH_SIZE * 4.0, SWATCH_SIZE * 1.5),
            egui::Sense::hover(),
        );
        let before = v.original.unwrap_or(v.value);
        let (left, right) = rect.split_left_right_at_fraction(0.5);
        ui.painter().rect_filled(left, 0.0, colour32(before));
        ui.painter().rect_filled(right, 0.0, colour32(v.value));
        let hex = hex_of(v.value);
        a11y::set_label(ui.ctx(), r.id, format!("Colour preview {hex}"));
        ui.label(hex);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CommandSink, UiModel};
    use crate::theme::{ResolvedTheme, ThemeTokens};
    use xarast_color::ColourTable;

    fn palette() -> Vec<PaletteEntry> {
        let mut v = vec![PaletteEntry::none()];
        for i in 0..512 {
            let t = i as f32 / 512.0;
            v.push(PaletteEntry::colour(
                format!("Swatch {i}"),
                ColourValue::rgb(t, 1.0 - t, 0.5),
            ));
        }
        v[3].named = true;
        v
    }

    fn view(model: ColourModel, components: [f32; 4]) -> ColourEditorView {
        ColourEditorView {
            target: ColourTarget::Selection(PaintSlot::Fill),
            title: "Fill of 1 object".to_owned(),
            model,
            model_editable: true,
            components,
            editable: [true; 4],
            value: ColourValue::from_components(model, components),
            original: None,
            dragging: false,
            linked_entry: None,
            entry: None,
            named: Vec::new(),
            objects: 1,
        }
    }

    fn run_frames(
        panel: &mut ColourPanel,
        model: &UiModel,
        inputs: Vec<egui::RawInput>,
    ) -> Vec<Vec<UiCommand>> {
        let tokens = ThemeTokens::of(ResolvedTheme::Dark);
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        for input in inputs {
            let mut sink = CommandSink::new();
            {
                let mut pctx = PanelCtx {
                    model,
                    tokens: &tokens,
                    out: &mut sink,
                };
                let _ = ctx.run(input, |c| {
                    egui::CentralPanel::default().show(c, |ui| panel.ui(ui, &mut pctx));
                });
            }
            out.push(sink.drain());
        }
        out
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 640.0),
            )),
            events,
            ..Default::default()
        }
    }

    #[test]
    fn a_full_palette_draws_without_a_command() {
        let model = UiModel {
            palette: palette(),
            colour_editor: Some(view(ColourModel::Rgbt, [1.0, 0.0, 0.0, 0.0])),
            ..Default::default()
        };
        let mut panel = ColourPanel::new();
        let out = run_frames(&mut panel, &model, vec![input(vec![]), input(vec![])]);
        assert!(out.iter().all(Vec::is_empty), "{out:?}");
    }

    #[test]
    fn without_a_document_the_panel_still_draws_the_line() {
        let model = UiModel {
            palette: palette(),
            ..Default::default()
        };
        let mut panel = ColourPanel::new();
        let out = run_frames(&mut panel, &model, vec![input(vec![])]);
        assert!(out[0].is_empty());
    }

    #[test]
    fn a_press_drag_release_on_the_field_previews_then_commits_once() {
        let model = UiModel {
            colour_editor: Some(view(ColourModel::Hsvt, [0.5, 0.5, 0.5, 0.0])),
            ..Default::default()
        };
        let mut panel = ColourPanel::new();
        // Find the field by its accessible name, not by guessing the
        // layout.
        let mut rect = None;
        {
            let tokens = ThemeTokens::of(ResolvedTheme::Dark);
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut sink = CommandSink::new();
            let out = ctx.run(input(vec![]), |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    let mut pctx = PanelCtx {
                        model: &model,
                        tokens: &tokens,
                        out: &mut sink,
                    };
                    panel.ui(ui, &mut pctx);
                });
            });
            if let Some(update) = out.platform_output.accesskit_update {
                for (_, node) in update.nodes {
                    if node.label().is_some_and(|l| l.contains("(across)"))
                        && let Some(b) = node.bounds()
                    {
                        rect = Some(egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        ));
                    }
                }
            }
        }
        let rect = rect.expect("the field is published with its name");
        let a = rect.center();
        let b = rect.right_top() + egui::vec2(-1.0, 1.0);
        let press = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = run_frames(
            &mut panel,
            &model,
            vec![
                input(vec![egui::Event::PointerMoved(a)]),
                input(vec![press(a, true)]),
                input(vec![egui::Event::PointerMoved(b)]),
                input(vec![press(b, false)]),
                input(vec![]),
            ],
        );
        let ops: Vec<&ColourEditorOp> = frames
            .iter()
            .flatten()
            .filter_map(|c| match c {
                UiCommand::ColourEditor(op) => Some(op),
                _ => None,
            })
            .collect();
        let previews = ops
            .iter()
            .filter(|o| matches!(o, ColourEditorOp::Preview(_)))
            .count();
        let commits = ops
            .iter()
            .filter(|o| matches!(o, ColourEditorOp::Commit))
            .count();
        assert!(previews >= 2, "{ops:?}");
        assert_eq!(commits, 1, "{ops:?}");
        assert!(matches!(ops.last(), Some(ColourEditorOp::Commit)));
        // The last preview is the top right: full saturation and value,
        // hue untouched.
        let last = ops
            .iter()
            .rev()
            .find_map(|o| match o {
                ColourEditorOp::Preview(ColourChange::Components(c)) => Some(*c),
                _ => None,
            })
            .unwrap();
        assert!((last[0] - 0.5).abs() < 1e-6, "{last:?}");
        assert!(last[1] > 0.95 && last[2] > 0.95, "{last:?}");
    }

    #[test]
    fn names_and_counts_follow_the_specs() {
        assert_eq!(
            component_names(ColourModel::Greyt),
            ["Grey", "Transparency", "", ""]
        );
        assert_eq!(component_count(ColourModel::Greyt), 2);
        assert_eq!(component_count(ColourModel::Cmyk), 4);
        for m in MODELS {
            assert!(!model_label(m).is_empty());
        }
    }

    #[test]
    fn a_new_colour_gets_the_first_free_name() {
        let mut t = ColourTable::new();
        let id = t.insert(xarast_color::ColourDef::normal(ColourValue::BLACK));
        let n = |name: &str| NamedColour {
            id,
            name: name.to_owned(),
            value: ColourValue::BLACK,
            can_parent: true,
        };
        assert_eq!(fresh_name(&[]), "Colour 1");
        assert_eq!(fresh_name(&[n("Colour 1"), n("Colour 3")]), "Colour 2");
        assert_eq!(target_label(ColourTarget::Entry(id), &[n("Sky")]), "Sky");
        assert_eq!(
            target_label(ColourTarget::Selection(PaintSlot::Stroke), &[]),
            "Selection line"
        );
    }

    #[test]
    fn no_colour_is_not_white_and_not_transparent_black() {
        let entry = PaletteEntry::none();
        assert!(swatch_colour(&entry).is_none());
        let white = PaletteEntry::colour("White", ColourValue::rgb(1.0, 1.0, 1.0));
        assert_eq!(
            swatch_colour(&white),
            Some(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 255))
        );
    }

    #[test]
    fn the_hexadecimal_form_matches_the_colour() {
        assert_eq!(hex_of(ColourValue::rgb(1.0, 0.0, 0.0)), "#FF0000");
        assert_eq!(hex_of(ColourValue::rgb(0.0, 0.0, 0.0)), "#000000");
    }

    #[test]
    fn transparency_reaches_the_swatch_alpha() {
        let half = ColourValue::rgbt(1.0, 0.0, 0.0, 0.5);
        let c = swatch_colour(&PaletteEntry::colour("Half", half)).unwrap();
        assert!(c.a() > 100 && c.a() < 160, "alpha {}", c.a());
    }
}
