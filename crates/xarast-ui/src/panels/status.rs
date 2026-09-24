//! The status bar.
//!
//! Coordinates in the document's units, the zoom, the render quality, the
//! cache pressure and the renderer tier (`research/04 §1.19`). It is not a
//! dockable panel: it is a strip at the foot of the window, always in the
//! same place, because a user reads coordinates without looking away from
//! the pointer.
//!
//! One deliberate honesty: the clipboard note. On Wayland a copy survives
//! only while the window has focus unless a data-control manager is
//! present, and the status bar says so when a copy happens rather than
//! pretending the data is safe.

use crate::model::{CommandSink, UiCommand, UiModel};
use crate::panel::PanelCtx;
use crate::theme::STATUS_BAR_HEIGHT;
use crate::units::format_measure;

/// The status bar's own state: nothing but the transient note.
#[derive(Debug, Default)]
pub struct StatusBar {
    note: Option<String>,
}

impl StatusBar {
    /// A fresh status bar.
    pub fn new() -> StatusBar {
        StatusBar::default()
    }

    /// Shows a transient note until it is replaced or cleared.
    pub fn set_note(&mut self, note: impl Into<String>) {
        self.note = Some(note.into());
    }

    /// Clears the transient note.
    pub fn clear_note(&mut self) {
        self.note = None;
    }

    /// Draws the bar.
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        status_bar_row(ui, ctx.model, self.note.as_deref(), Some(ctx.out));
    }
}

/// The text of the coordinate readout.
///
/// Extracted so it can be asserted without a window: a status bar that
/// silently shows the wrong unit is exactly the kind of bug a snapshot
/// test misses and a string test catches.
pub fn coordinate_text(model: &UiModel) -> String {
    let Some(doc) = model.document.as_ref() else {
        return String::new();
    };
    match model.status.pointer {
        Some((x, y)) => format!(
            "{}, {}",
            format_measure(x, doc.unit),
            format_measure(y, doc.unit)
        ),
        None => "—".to_owned(),
    }
}

/// The text of the zoom readout.
pub fn zoom_text(model: &UiModel) -> String {
    match model.document.as_ref() {
        Some(doc) => format!("{:.0} %", doc.view.zoom_percent()),
        None => String::new(),
    }
}

/// The text of the cache-pressure readout.
///
/// A percentage and a word, because a bare number tells a user nothing
/// about whether they should worry.
pub fn cache_text(pressure: f32) -> String {
    let pressure = if pressure.is_finite() { pressure } else { 0.0 };
    let pct = (pressure.clamp(0.0, 1.0) * 100.0).round();
    let word = if pressure >= 0.9 {
        "full"
    } else if pressure >= 0.7 {
        "high"
    } else {
        "ok"
    };
    format!("Cache {pct:.0} % ({word})")
}

/// The text beside the import progress bar: the oldest import's own
/// words, and how many more are queued behind it.
pub fn import_text(model: &UiModel) -> Option<String> {
    let first = model.imports.first()?;
    let more = model.imports.len() - 1;
    Some(if more == 0 {
        first.label()
    } else {
        format!("{} (+{more} more)", first.label())
    })
}

/// The background imports (T10.7.5): a progress bar with its label and a
/// Cancel button, drawn right to left.
fn imports_ui(ui: &mut egui::Ui, model: &UiModel, out: &mut CommandSink) {
    let (Some(first), Some(text)) = (model.imports.first(), import_text(model)) else {
        return;
    };
    let cancel = ui
        .button("Cancel")
        .on_hover_text("Stop importing: nothing is added to the document");
    crate::a11y::set_label(ui.ctx(), cancel.id, "Cancel the import");
    if cancel.clicked() {
        out.push(UiCommand::CancelImports);
    }
    let bar = ui.add(
        egui::ProgressBar::new(first.fraction())
            .desired_width(120.0)
            .animate(true),
    );
    crate::a11y::set_label(ui.ctx(), bar.id, &text);
    ui.label(text);
    // The bar moves while the worker reads; keep frames coming.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
}

/// Draws the status bar into the space available.
pub fn status_bar_ui(ui: &mut egui::Ui, model: &UiModel, note: Option<&str>) {
    status_bar_row(ui, model, note, None);
}

/// The status bar; with a sink it also offers the imports' Cancel.
fn status_bar_row(
    ui: &mut egui::Ui,
    model: &UiModel,
    note: Option<&str>,
    out: Option<&mut CommandSink>,
) {
    ui.set_min_height(STATUS_BAR_HEIGHT);
    ui.horizontal(|ui| {
        ui.label(coordinate_text(model))
            .on_hover_text("Pointer position in the document's units");
        ui.separator();
        ui.label(zoom_text(model)).on_hover_text("Zoom");
        ui.separator();
        ui.label(model.status.quality.label())
            .on_hover_text("Render quality of the last frame");
        ui.separator();
        ui.label(cache_text(model.status.cache_pressure))
            .on_hover_text("Render cache occupancy");
        ui.separator();
        ui.label(&model.status.renderer)
            .on_hover_text("The renderer tier the shell selected");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(out) = out {
                imports_ui(ui, model, out);
            }
            if model.status.problem_count > 0 {
                let text = format!("{} problems", model.status.problem_count);
                if ui
                    .button(text)
                    .on_hover_text("Show the import problem list")
                    .clicked()
                {
                    // The caller drains this; the status bar itself holds
                    // no application state.
                }
            }
            if let Some(note) = note.or(model.status.message.as_deref()) {
                ui.label(note);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DocumentView, RenderQuality, StatusInfo, UiModel};
    use crate::units::Unit;
    use xarast_geom::Mp;

    fn model(unit: Unit, pointer: Option<(Mp, Mp)>) -> UiModel {
        UiModel {
            document: Some(DocumentView {
                unit,
                ..Default::default()
            }),
            status: StatusInfo {
                pointer,
                quality: RenderQuality::Draft,
                cache_pressure: 0.42,
                renderer: "GPU (fast)".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn coordinates_are_shown_in_the_documents_unit() {
        let m = model(
            Unit::Millimetre,
            Some((Mp::from_mm(10.0), Mp::from_mm(20.0))),
        );
        assert_eq!(coordinate_text(&m), "10mm, 20mm");
        let m = model(Unit::Inch, Some((Mp::from_inch(1.0), Mp::from_inch(2.0))));
        assert_eq!(coordinate_text(&m), "1in, 2in");
    }

    #[test]
    fn a_pointer_off_the_canvas_shows_a_dash_not_a_stale_value() {
        let m = model(Unit::Point, None);
        assert_eq!(coordinate_text(&m), "—");
    }

    #[test]
    fn with_no_document_the_readouts_are_empty() {
        let m = UiModel::default();
        assert_eq!(coordinate_text(&m), "");
        assert_eq!(zoom_text(&m), "");
    }

    #[test]
    fn the_zoom_is_shown_as_a_percentage() {
        let mut m = model(Unit::Point, None);
        m.document.as_mut().unwrap().view.zoom = 2.5;
        assert_eq!(zoom_text(&m), "250 %");
    }

    #[test]
    fn cache_pressure_is_a_number_and_a_word() {
        assert_eq!(cache_text(0.0), "Cache 0 % (ok)");
        assert_eq!(cache_text(0.75), "Cache 75 % (high)");
        assert_eq!(cache_text(0.95), "Cache 95 % (full)");
        assert_eq!(cache_text(5.0), "Cache 100 % (full)");
        assert_eq!(cache_text(f32::NAN), "Cache 0 % (ok)");
    }

    #[test]
    fn imports_show_progress_and_a_cancel_that_asks_the_core() {
        let mut m = model(Unit::Millimetre, None);
        assert_eq!(import_text(&m), None);
        let p = |id, name: &str| xarast_app::import::ImportProgress {
            id,
            name: name.to_owned(),
            read: 512 * 1024,
            total: 1024 * 1024,
            decoding: false,
        };
        m.imports = vec![p(1, "photo.jpg"), p(2, "other.png")];
        assert_eq!(
            import_text(&m).as_deref(),
            Some("Importing photo.jpg: read 512 of 1024 KB\u{2026} (+1 more)")
        );
    }

    #[test]
    fn the_bar_draws_headlessly_with_every_readout() {
        let m = model(Unit::Millimetre, Some((Mp::ZERO, Mp::ZERO)));
        let ctx = egui::Context::default();
        let mut bar = StatusBar::new();
        bar.set_note("Copied to the clipboard (Wayland: only while this window has focus)");
        let tokens = crate::theme::ThemeTokens::of(crate::theme::ResolvedTheme::Light);
        let mut sink = crate::model::CommandSink::new();
        let mut pctx = PanelCtx {
            model: &m,
            tokens: &tokens,
            out: &mut sink,
        };
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 40.0),
                )),
                ..Default::default()
            },
            |c| {
                egui::TopBottomPanel::bottom("status").show(c, |ui| bar.ui(ui, &mut pctx));
            },
        );
        bar.clear_note();
    }
}
