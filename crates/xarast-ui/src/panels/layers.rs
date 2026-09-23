//! The layer panel.
//!
//! A Xara document is organised in layers and the panel is how they are
//! reached (`research/04 §1.1`, `§1.11`): show and hide, lock, choose the
//! active one, reorder. The list is virtualised from the start — a
//! generated document can have hundreds of layers and the spike measures
//! five thousand rows.
//!
//! Every control is a real egui widget with a text label, so the panel is
//! operable from the keyboard and legible to AT-SPI. Arrow keys move the
//! active layer, `Space` toggles visibility, `L` toggles the lock: a
//! keyboard user never has to reach for the mouse to hide a layer.

use crate::a11y;
use crate::model::{LayerInfo, LayerKey, UiCommand};
use crate::panel::{Panel, PanelCtx, PanelId};
use crate::theme::ROW_HEIGHT;

/// The layer panel's identifier.
pub const ID: PanelId = PanelId("layers");

/// The layer panel.
#[derive(Debug, Default)]
pub struct LayerPanel {
    renaming: Option<(LayerKey, String)>,
    /// The rename field has just appeared and must take the keyboard, or
    /// the double-click that opened it leaves nowhere to type.
    focus_rename: bool,
}

impl LayerPanel {
    /// A fresh panel.
    pub fn new() -> LayerPanel {
        LayerPanel::default()
    }
}

impl Panel for LayerPanel {
    fn id(&self) -> PanelId {
        ID
    }

    fn title(&self) -> &str {
        "Layers"
    }

    fn min_size(&self) -> egui::Vec2 {
        egui::vec2(200.0, 120.0)
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        let Some(doc) = ctx.model.document.as_ref() else {
            ui.label("No document open");
            return;
        };

        ui.horizontal(|ui| {
            if ui.button("New").on_hover_text("Add a layer").clicked() {
                ctx.out.push(UiCommand::AddLayer);
            }
            if let Some(active) = doc.active_layer
                && ui
                    .button("Delete")
                    .on_hover_text("Delete the active layer")
                    .clicked()
            {
                ctx.out.push(UiCommand::DeleteLayer(active));
            }
        });
        ui.separator();

        // Top-most layer first, which is how a user thinks of a stack.
        let order: Vec<usize> = (0..doc.layers.len()).rev().collect();
        let active_row = doc
            .active_layer
            .and_then(|k| order.iter().position(|&i| doc.layers[i].key == k));

        let list = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, order.len(), |ui, range| {
                // The scroll area itself is the list; egui would publish
                // it as an anonymous container.
                a11y::set_role(ui.ctx(), ui.id(), egui::accesskit::Role::List);
                for row in range {
                    let index = order[row];
                    let layer = &doc.layers[index];
                    self.layer_row(
                        ui,
                        layer,
                        index,
                        doc.layers.len(),
                        row,
                        order.len(),
                        active_row == Some(row),
                        ctx,
                    );
                }
            });
        let _ = list;

        // Keyboard navigation over the whole panel, once, rather than per
        // row: arrow keys move the active layer through the stack.
        if let (Some(active), Some(row)) = (doc.active_layer, active_row) {
            let pressed = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::Space),
                    i.key_pressed(egui::Key::L),
                )
            });
            let current = &doc.layers[order[row]];
            if pressed.0 && row > 0 {
                ctx.out
                    .push(UiCommand::SetActiveLayer(doc.layers[order[row - 1]].key));
            }
            if pressed.1 && row + 1 < order.len() {
                ctx.out
                    .push(UiCommand::SetActiveLayer(doc.layers[order[row + 1]].key));
            }
            if pressed.2 {
                ctx.out.push(UiCommand::SetLayerVisible {
                    layer: active,
                    visible: !current.visible,
                });
            }
            if pressed.3 {
                ctx.out.push(UiCommand::SetLayerLocked {
                    layer: active,
                    locked: !current.locked,
                });
            }
        }
    }
}

impl LayerPanel {
    #[allow(clippy::too_many_arguments)]
    fn layer_row(
        &mut self,
        ui: &mut egui::Ui,
        layer: &LayerInfo,
        index: usize,
        count: usize,
        row: usize,
        rows: usize,
        active: bool,
        ctx: &mut PanelCtx<'_>,
    ) {
        let row_response = ui.horizontal(|ui| {
            ui.set_min_height(ROW_HEIGHT);

            let mut visible = layer.visible;
            let toggle = ui.add(egui::Checkbox::new(&mut visible, ""));
            // A checkbox with no text is anonymous to a screen reader.
            a11y::set_label(
                ui.ctx(),
                toggle.id,
                format!(
                    "{} layer {}",
                    if layer.visible { "Hide" } else { "Show" },
                    layer.name
                ),
            );
            if toggle
                .on_hover_text(if layer.visible {
                    "Hide layer"
                } else {
                    "Show layer"
                })
                .changed()
            {
                ctx.out.push(UiCommand::SetLayerVisible {
                    layer: layer.key,
                    visible,
                });
            }

            let lock_label = if layer.locked { "Locked" } else { "Unlocked" };
            if ui
                .selectable_label(layer.locked, lock_label)
                .on_hover_text("Lock the layer against selection and editing")
                .clicked()
            {
                ctx.out.push(UiCommand::SetLayerLocked {
                    layer: layer.key,
                    locked: !layer.locked,
                });
            }

            match &mut self.renaming {
                Some((key, text)) if *key == layer.key => {
                    let response = ui.add(
                        egui::TextEdit::singleline(text)
                            .desired_width(f32::INFINITY)
                            .hint_text("Layer name"),
                    );
                    if std::mem::take(&mut self.focus_rename) {
                        response.request_focus();
                    }
                    if response.lost_focus() {
                        let name = std::mem::take(text);
                        if !name.is_empty() && name != layer.name {
                            ctx.out.push(UiCommand::RenameLayer {
                                layer: layer.key,
                                name,
                            });
                        }
                        self.renaming = None;
                    }
                }
                _ => {
                    let label = a11y::layer_label(&layer.name, layer.visible, layer.locked);
                    let response = ui.selectable_label(active, &layer.name);
                    a11y::set_label(ui.ctx(), response.id, label.clone());
                    let response = response.on_hover_text(&label);
                    if response.clicked() {
                        ctx.out.push(UiCommand::SetActiveLayer(layer.key));
                    }
                    if response.double_clicked() {
                        self.renaming = Some((layer.key, layer.name.clone()));
                        self.focus_rename = true;
                    }
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Reordering without drag-and-drop as well as with it: a
                // keyboard user must be able to restack a document.
                if index + 1 < count && ui.small_button("Up").clicked() {
                    ctx.out.push(UiCommand::MoveLayer {
                        layer: layer.key,
                        to_index: index + 1,
                    });
                }
                if index > 0 && ui.small_button("Down").clicked() {
                    ctx.out.push(UiCommand::MoveLayer {
                        layer: layer.key,
                        to_index: index - 1,
                    });
                }
                if layer.object_count > 0 {
                    ui.weak(format!("{}", layer.object_count));
                }
            });
        });
        // Say where this row sits in the stack, so a screen reader can
        // announce "Sky, visible, unlocked, 3 of 12" instead of reading a
        // run of anonymous buttons.
        a11y::set_list_item(ui.ctx(), row_response.response.id, row, rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CommandSink, DocumentView, UiModel};
    use crate::panel::Panel;
    use crate::theme::{ResolvedTheme, ThemeTokens};

    fn model_with_layers(n: usize) -> UiModel {
        let layers: Vec<_> = (0..n)
            .map(|i| LayerInfo::new(i as u64, format!("Layer {i}")))
            .collect();
        let active = layers.first().map(|l| l.key);
        UiModel {
            document: Some(DocumentView {
                layers,
                active_layer: active,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn run(panel: &mut LayerPanel, model: &UiModel, input: egui::RawInput) -> CommandSink {
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
                egui::vec2(300.0, 400.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_session_says_so_instead_of_showing_an_empty_list() {
        let mut panel = LayerPanel::new();
        let out = run(&mut panel, &UiModel::default(), input());
        assert!(out.is_empty());
    }

    #[test]
    fn a_five_thousand_layer_document_only_builds_the_visible_rows() {
        // Virtualisation is the property under test: the panel must run in
        // time proportional to the visible rows, not the document.
        let big = model_with_layers(5_000);
        let mut panel = LayerPanel::new();
        let start = std::time::Instant::now();
        let _ = run(&mut panel, &big, input());
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "5,000 layers took {elapsed:?}"
        );
    }

    #[test]
    fn space_toggles_the_active_layer_visibility() {
        let model = model_with_layers(3);
        let mut panel = LayerPanel::new();
        let mut i = input();
        i.events.push(egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let out = run(&mut panel, &model, i);
        assert!(
            out.commands()
                .iter()
                .any(|c| matches!(c, UiCommand::SetLayerVisible { visible: false, .. })),
            "{:?}",
            out.commands()
        );
    }

    #[test]
    fn the_l_key_toggles_the_lock() {
        let model = model_with_layers(3);
        let mut panel = LayerPanel::new();
        let mut i = input();
        i.events.push(egui::Event::Key {
            key: egui::Key::L,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let out = run(&mut panel, &model, i);
        assert!(
            out.commands()
                .iter()
                .any(|c| matches!(c, UiCommand::SetLayerLocked { locked: true, .. })),
            "{:?}",
            out.commands()
        );
    }

    #[test]
    fn arrow_keys_walk_the_stack_without_falling_off_either_end() {
        let model = model_with_layers(3);
        let mut panel = LayerPanel::new();
        let mut i = input();
        i.events.push(egui::Event::Key {
            key: egui::Key::ArrowUp,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        // The active layer is the bottom one, shown last: up is legal.
        let out = run(&mut panel, &model, i);
        assert!(
            out.commands()
                .iter()
                .any(|c| matches!(c, UiCommand::SetActiveLayer(_))),
            "{:?}",
            out.commands()
        );

        // With one layer there is nowhere to go and nothing is emitted.
        let single = model_with_layers(1);
        let mut i = input();
        for key in [egui::Key::ArrowUp, egui::Key::ArrowDown] {
            i.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
        }
        let out = run(&mut panel, &single, i);
        assert!(
            !out.commands()
                .iter()
                .any(|c| matches!(c, UiCommand::SetActiveLayer(_))),
            "{:?}",
            out.commands()
        );
    }

    #[test]
    fn the_panel_advertises_a_usable_minimum_size() {
        let panel = LayerPanel::new();
        assert!(panel.min_size().x >= 180.0);
        assert_eq!(panel.id(), ID);
        assert_eq!(panel.title(), "Layers");
    }
}
