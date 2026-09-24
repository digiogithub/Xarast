//! The colour gallery (phase 8, T8.7.5): the document's named colours as a
//! tree of derivations — a tint, shade or link sits under the colour it
//! derives from — with create, edit, rename and delete, and every swatch a
//! colour that can be dragged onto an object, a gradient stop or another
//! named colour.
//!
//! It draws the same [`xarast_app::colour_bar::ColourBarView`] as the colour
//! bar and answers with the same operations; "Edit" hands the colour to the
//! colour editor ([`ColourEditorOp::SetTarget`]).

use xarast_app::colour_bar::{ColourBarOp, ColourSource, Swatch};
use xarast_app::colour_editor::{ColourEditorOp, ColourTarget};
use xarast_color::ColourId;

use crate::a11y;
use crate::colour_bar::{
    click_slot, drive_drag, fresh_name, paint_swatch, publish_swatch, register_slot, swatches,
};
use crate::model::UiCommand;
use crate::panel::{Panel, PanelCtx, PanelId};
use crate::theme::SWATCH_SIZE;

/// The colour gallery's identifier.
pub const ID: PanelId = PanelId("colour_gallery");

/// How far a derived colour is indented under its parent, in points.
const INDENT: f32 = 14.0;

/// The colour gallery. It keeps interaction state only: the colour chosen
/// in the list and a name being typed.
#[derive(Debug, Default)]
pub struct ColourGallery {
    chosen: Option<ColourId>,
    renaming: Option<(ColourId, String)>,
}

impl ColourGallery {
    /// A fresh gallery.
    pub fn new() -> ColourGallery {
        ColourGallery::default()
    }
}

/// The named swatches in tree order: each followed by the colours derived
/// from it, with their depth. A colour whose parent is not listed is a
/// root.
pub fn tree(swatches: &[Swatch]) -> Vec<(usize, &Swatch)> {
    let named: Vec<&Swatch> = swatches.iter().filter(|s| s.named).collect();
    let id_of = |s: &Swatch| match s.source {
        ColourSource::Named(id) => Some(id),
        _ => None,
    };
    let listed = |id: ColourId| named.iter().any(|s| id_of(s) == Some(id));
    let mut out = Vec::with_capacity(named.len());
    let mut stack: Vec<(usize, &Swatch)> = named
        .iter()
        .rev()
        .filter(|s| !s.parent.is_some_and(listed))
        .map(|s| (0, *s))
        .collect();
    while let Some((depth, s)) = stack.pop() {
        out.push((depth, s));
        if out.len() > named.len() {
            break; // a loop: never from the application, but never hang
        }
        let Some(id) = id_of(s) else { continue };
        stack.extend(
            named
                .iter()
                .rev()
                .filter(|c| c.parent == Some(id))
                .map(|c| (depth + 1, *c)),
        );
    }
    out
}

impl Panel for ColourGallery {
    fn id(&self) -> PanelId {
        ID
    }

    fn title(&self) -> &str {
        "Colour gallery"
    }

    fn min_size(&self) -> egui::Vec2 {
        egui::vec2(200.0, 140.0)
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        if ctx.model.document.is_none() {
            ui.label("Open a document to see its colours.");
            return;
        }
        let all = swatches(ctx.model);
        let rows = tree(&all);
        if let Some(c) = self.chosen
            && !rows.iter().any(|(_, s)| s.source == ColourSource::Named(c))
        {
            self.chosen = None;
        }
        self.buttons(ui, &all, ctx);
        ui.separator();
        if rows.is_empty() {
            ui.label("This document has no named colours yet.");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("xarast_colour_gallery")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (depth, s) in rows {
                    self.row(ui, depth, s, ctx);
                }
            });
    }
}

impl ColourGallery {
    fn buttons(&mut self, ui: &mut egui::Ui, all: &[Swatch], ctx: &mut PanelCtx<'_>) {
        let chosen = self.chosen;
        let name = |id: ColourId| {
            all.iter()
                .find(|s| s.source == ColourSource::Named(id))
                .map_or_else(String::new, |s| s.name.clone())
        };
        ui.horizontal_wrapped(|ui| {
            // The buttons say "New", "Edit", …; their accessible names say
            // what they act on, and stay distinct from the Edit menu's.
            let new = ui
                .button("New")
                .on_hover_text("Make a named colour from the colour editor's colour");
            a11y::set_label(ui.ctx(), new.id, "New named colour");
            if new.clicked() {
                ctx.out
                    .push(UiCommand::ColourEditor(ColourEditorOp::NewNamed(
                        fresh_name(all),
                    )));
            }
            let edit = ui
                .add_enabled(chosen.is_some(), egui::Button::new("Edit"))
                .on_hover_text("Edit the chosen colour in the colour editor");
            a11y::set_label(ui.ctx(), edit.id, "Edit the chosen colour");
            if edit.clicked()
                && let Some(id) = chosen
            {
                ctx.out
                    .push(UiCommand::ColourEditor(ColourEditorOp::SetTarget(
                        ColourTarget::Entry(id),
                    )));
            }
            let rename = ui.add_enabled(chosen.is_some(), egui::Button::new("Rename"));
            a11y::set_label(ui.ctx(), rename.id, "Rename the chosen colour");
            if rename.clicked()
                && let Some(id) = chosen
            {
                self.renaming = Some((id, name(id)));
            }
            let delete = ui
                .add_enabled(chosen.is_some(), egui::Button::new("Delete"))
                .on_hover_text("Delete the chosen colour: objects using it keep their look");
            a11y::set_label(ui.ctx(), delete.id, "Delete the chosen colour");
            if delete.clicked()
                && let Some(id) = chosen
            {
                ctx.out.push(UiCommand::ColourBar(ColourBarOp::Delete(id)));
                self.chosen = None;
            }
        });
    }

    fn row(&mut self, ui: &mut egui::Ui, depth: usize, s: &Swatch, ctx: &mut PanelCtx<'_>) {
        let ColourSource::Named(id) = s.source else {
            return;
        };
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * INDENT);
            let (rect, r) = ui.allocate_exact_size(
                egui::vec2(SWATCH_SIZE, SWATCH_SIZE),
                egui::Sense::click_and_drag(),
            );
            publish_swatch(&r, s);
            paint_swatch(ui.painter(), rect, s, ctx.tokens);
            register_slot(ui.ctx(), rect, id);
            let r = r.on_hover_text("Click: fill; right click: line; drag onto an object");
            if let Some(slot) = click_slot(ui, &r) {
                self.chosen = Some(id);
                ctx.out.push(UiCommand::ColourBar(ColourBarOp::Apply {
                    source: s.source,
                    slot,
                }));
            }
            drive_drag(ui, &r, s.source, ctx.model, ctx.out);

            match &mut self.renaming {
                Some((rid, buf)) if *rid == id => {
                    let t = ui.add(egui::TextEdit::singleline(buf).desired_width(120.0));
                    a11y::set_label(ui.ctx(), t.id, "Colour name");
                    if !t.has_focus() && !t.lost_focus() {
                        t.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.renaming = None;
                    } else if t.lost_focus() {
                        let name = buf.trim().to_owned();
                        self.renaming = None;
                        if !name.is_empty() && name != s.name {
                            ctx.out
                                .push(UiCommand::ColourBar(ColourBarOp::Rename { id, name }));
                        }
                    }
                }
                _ => {
                    let label = if s.kind == "Normal" || s.kind.is_empty() {
                        s.name.clone()
                    } else {
                        format!("{} ({})", s.name, s.kind.to_lowercase())
                    };
                    let row = ui.selectable_label(self.chosen == Some(id), label);
                    register_slot(ui.ctx(), row.rect, id);
                    if row.clicked() {
                        self.chosen = Some(id);
                    }
                    if row.double_clicked() {
                        ctx.out
                            .push(UiCommand::ColourEditor(ColourEditorOp::SetTarget(
                                ColourTarget::Entry(id),
                            )));
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_color::{ColourDef, ColourTable, ColourValue};

    fn sw(id: ColourId, name: &str, parent: Option<ColourId>) -> Swatch {
        Swatch {
            source: ColourSource::Named(id),
            name: name.to_owned(),
            value: Some(ColourValue::BLACK),
            named: true,
            parent,
            kind: if parent.is_some() { "Tint" } else { "Normal" },
        }
    }

    #[test]
    fn derived_colours_sit_under_their_parents() {
        let mut t = ColourTable::new();
        let mut id = || t.insert(ColourDef::normal(ColourValue::BLACK));
        let (a, b, c, d) = (id(), id(), id(), id());
        let list = vec![
            sw(c, "C of A", Some(a)),
            sw(a, "A", None),
            sw(b, "B", None),
            sw(d, "D of C", Some(c)),
        ];
        let rows: Vec<(usize, &str)> = tree(&list)
            .into_iter()
            .map(|(d, s)| (d, s.name.as_str()))
            .collect();
        assert_eq!(rows, vec![(0, "A"), (1, "C of A"), (2, "D of C"), (0, "B")]);
    }
}
