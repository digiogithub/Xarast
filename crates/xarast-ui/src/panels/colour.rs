//! The colour panel and the on-screen colour line.
//!
//! Two things in one panel, as `research/04 §1.5` and `§2.2` describe them:
//!
//! * the **colour line**, a scrolling strip of swatches along the bottom of
//!   the window, where a click sets the fill and a right-click the line
//!   colour, and where the first entry is "no colour" — which removes the
//!   attribute rather than painting white;
//! * the **colour editor**, showing the current colour in RGB, HSV or grey,
//!   with the same value in every model because the conversion is
//!   `xarast-color`'s, not the panel's.
//!
//! Named document colours are marked: editing one changes every object
//! that uses it, which is the differentiator `research/04 §3` item 10 calls
//! out, and a user needs to be told which colours behave that way.

use xarast_color::{ColourModel, ColourValue};

use crate::a11y;
use crate::model::{PaletteEntry, UiCommand};
use crate::panel::{Panel, PanelCtx, PanelId};
use crate::theme::SWATCH_SIZE;

/// The colour panel's identifier.
pub const ID: PanelId = PanelId("colour");

/// The colour panel.
#[derive(Debug)]
pub struct ColourPanel {
    model: ColourModel,
    editing_line: bool,
}

impl Default for ColourPanel {
    fn default() -> Self {
        ColourPanel {
            model: ColourModel::Rgbt,
            editing_line: false,
        }
    }
}

impl ColourPanel {
    /// A fresh panel, editing the fill colour in RGB.
    pub fn new() -> ColourPanel {
        ColourPanel::default()
    }

    /// Which colour model the editor is showing.
    pub fn colour_model(&self) -> ColourModel {
        self.model
    }

    /// Whether the editor is editing the line colour rather than the fill.
    pub fn editing_line(&self) -> bool {
        self.editing_line
    }
}

/// The component names of a colour model, in order.
///
/// Four names always, with the fourth being the transparency `xarast-color`
/// carries in every model.
pub fn component_names(model: ColourModel) -> [&'static str; 4] {
    match model {
        ColourModel::Rgbt => ["Red", "Green", "Blue", "Transparency"],
        ColourModel::Cmyk => ["Cyan", "Magenta", "Yellow", "Key"],
        ColourModel::Hsvt => ["Hue", "Saturation", "Value", "Transparency"],
        ColourModel::Greyt => ["Grey", "", "", "Transparency"],
        ColourModel::Ciet => ["X", "Y", "Z", "Transparency"],
        ColourModel::Indexed | ColourModel::WebRgbt => ["Red", "Green", "Blue", "Transparency"],
    }
}

/// How many components a model actually shows.
pub fn component_count(model: ColourModel) -> usize {
    match model {
        ColourModel::Greyt => 2,
        _ => 4,
    }
}

/// The swatch colour for a palette entry, as egui wants it.
///
/// "No colour" is drawn as a hollow swatch with a diagonal, never as white:
/// the distinction matters and a user must be able to see it.
pub fn swatch_colour(entry: &PaletteEntry) -> Option<egui::Color32> {
    entry.colour.map(|c| {
        let rgba = c.to_rgba8();
        egui::Color32::from_rgba_unmultiplied(rgba.r, rgba.g, rgba.b, rgba.a)
    })
}

impl Panel for ColourPanel {
    fn id(&self) -> PanelId {
        ID
    }

    fn title(&self) -> &str {
        "Colour"
    }

    fn min_size(&self) -> egui::Vec2 {
        egui::vec2(220.0, 140.0)
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.editing_line, false, "Fill");
            ui.selectable_value(&mut self.editing_line, true, "Line");
            ui.separator();
            for model in [
                ColourModel::Rgbt,
                ColourModel::Hsvt,
                ColourModel::Greyt,
                ColourModel::Cmyk,
            ] {
                ui.selectable_value(&mut self.model, model, model_label(model));
            }
        });

        let current = if self.editing_line {
            ctx.model.line
        } else {
            ctx.model.fill
        };

        ui.separator();
        self.editor(ui, current, ctx);
        ui.separator();
        colour_line(ui, ctx, self.editing_line);
    }
}

fn model_label(model: ColourModel) -> &'static str {
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

impl ColourPanel {
    fn editor(&mut self, ui: &mut egui::Ui, current: Option<ColourValue>, ctx: &mut PanelCtx<'_>) {
        let target = if self.editing_line { "line" } else { "fill" };
        let Some(colour) = current else {
            ui.horizontal(|ui| {
                ui.label(format!("No {target} colour"));
                if ui.button("Set to black").clicked() {
                    emit(
                        ctx,
                        self.editing_line,
                        Some(ColourValue::rgb(0.0, 0.0, 0.0)),
                    );
                }
            });
            return;
        };

        let shown = colour.to_model(self.model);
        let mut comps = shown.components();
        let names = component_names(self.model);
        let count = component_count(self.model);
        let mut changed = false;

        for i in 0..count {
            if names[i].is_empty() {
                continue;
            }
            // A slider, not a bare number: it is draggable, it is
            // focusable, and AccessKit publishes it with its name and its
            // value.
            let response = ui.add(
                egui::Slider::new(&mut comps[i], 0.0..=1.0)
                    .text(names[i])
                    .fixed_decimals(3),
            );
            changed |= response.changed();
        }

        ui.horizontal(|ui| {
            let preview = swatch_colour(&PaletteEntry {
                colour: Some(colour),
                name: String::new(),
                named: false,
            })
            .unwrap_or(egui::Color32::TRANSPARENT);
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(SWATCH_SIZE * 2.0, SWATCH_SIZE),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(rect, 2.0, preview);
            ui.label(hex_of(colour));
            // Named differently from the palette's "No colour" swatch on
            // purpose: two controls with the same accessible name in one
            // panel are indistinguishable to a screen reader.
            if ui
                .button("Remove colour")
                .on_hover_text("Remove the attribute rather than painting it white")
                .clicked()
            {
                emit(ctx, self.editing_line, None);
            }
        });

        if changed {
            emit(
                ctx,
                self.editing_line,
                Some(ColourValue::from_components(self.model, comps)),
            );
        }
    }
}

/// The hexadecimal form shown next to the preview.
pub fn hex_of(colour: ColourValue) -> String {
    let c = colour.to_rgba8();
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn emit(ctx: &mut PanelCtx<'_>, line: bool, colour: Option<ColourValue>) {
    ctx.out.push(if line {
        UiCommand::SetLine(colour)
    } else {
        UiCommand::SetFill(colour)
    });
}

/// The on-screen colour line: a scrolling strip of swatches.
///
/// Click sets the fill, right-click (or a click with the line tab active)
/// sets the line colour. Every swatch is a focusable widget with a name, so
/// the palette is reachable with `Tab` and `Enter`.
pub fn colour_line(ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>, editing_line: bool) {
    egui::ScrollArea::horizontal()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for entry in &ctx.model.palette {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(SWATCH_SIZE, SWATCH_SIZE),
                        egui::Sense::click(),
                    );
                    let label = a11y::swatch_label(&entry.name, entry.named);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
                    });
                    match swatch_colour(entry) {
                        Some(colour) => {
                            ui.painter().rect_filled(rect, 1.0, colour);
                        }
                        None => {
                            // "No colour": hollow, with a diagonal.
                            ui.painter().rect_stroke(
                                rect,
                                1.0,
                                egui::Stroke::new(1.0_f32, ctx.tokens.border),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().line_segment(
                                [rect.left_bottom(), rect.right_top()],
                                egui::Stroke::new(1.0_f32, ctx.tokens.error),
                            );
                        }
                    }
                    if entry.named {
                        ui.painter().rect_stroke(
                            rect,
                            1.0,
                            egui::Stroke::new(1.0_f32, ctx.tokens.accent),
                            egui::StrokeKind::Outside,
                        );
                    }
                    let response = response.on_hover_text(&entry.name);
                    if response.clicked() {
                        emit(ctx, editing_line, entry.colour);
                    }
                    if response.secondary_clicked() {
                        ctx.out.push(UiCommand::SetLine(entry.colour));
                    }
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CommandSink, UiModel};
    use crate::theme::{ResolvedTheme, ThemeTokens};

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

    fn run(panel: &mut ColourPanel, model: &UiModel, input: egui::RawInput) -> CommandSink {
        let tokens = ThemeTokens::of(ResolvedTheme::Dark);
        let mut sink = CommandSink::new();
        let ctx = egui::Context::default();
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
        sink
    }

    fn input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn a_full_palette_draws_without_a_command() {
        let model = UiModel {
            palette: palette(),
            fill: Some(ColourValue::rgb(1.0, 0.0, 0.0)),
            ..Default::default()
        };
        let mut panel = ColourPanel::new();
        let out = run(&mut panel, &model, input());
        assert!(out.is_empty(), "{:?}", out.commands());
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
    fn the_editor_offers_to_create_a_colour_when_there_is_none() {
        let model = UiModel {
            palette: palette(),
            fill: None,
            ..Default::default()
        };
        let mut panel = ColourPanel::new();
        let out = run(&mut panel, &model, input());
        assert!(out.is_empty());
        assert!(!panel.editing_line());
        assert_eq!(panel.colour_model(), ColourModel::Rgbt);
    }

    #[test]
    fn every_model_names_its_components() {
        for model in [
            ColourModel::Rgbt,
            ColourModel::Cmyk,
            ColourModel::Hsvt,
            ColourModel::Greyt,
            ColourModel::Ciet,
        ] {
            let names = component_names(model);
            assert!(!names[0].is_empty(), "{model:?}");
            assert!(component_count(model) >= 2, "{model:?}");
            assert!(!model_label(model).is_empty());
        }
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
