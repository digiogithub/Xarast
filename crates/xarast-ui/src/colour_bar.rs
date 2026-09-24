//! The on-screen colour bar (phase 8, W8.7): a strip of swatches below the
//! canvas, and the colour drag the bar and the colour gallery share.
//!
//! The bar draws [`xarast_app::colour_bar::ColourBarView`] and answers with
//! [`UiCommand::ColourBar`]: what a click or a drop *does* is decided in
//! `xarast-app`. It is one allocation painted by hand, with one
//! `ui.interact` per **visible** swatch — a palette of a few hundred colours
//! costs the dozens on screen, not hundreds of widgets — and every visible
//! swatch is still an AccessKit button named after its colour, with the
//! colour's `#RRGGBB` as its value.
//!
//! * **Left click**: the selection's fill. **Right click** or
//!   **`Shift`+click**: its line colour.
//! * **Drag**: pick the colour up. Over the canvas the shell resolves the
//!   drop target ([`UiCommand::ColourDragAt`]); over a named colour of the
//!   bar or the gallery, the drop reorders or redefines it. `Esc` cancels.
//! * **Paging**: the arrow buttons at each end, and the mouse wheel over
//!   the strip, scroll it a page (or three swatches a notch).
//! * **Menu**: a right click on the strip's empty part, or the menu button
//!   at its end, acts on the named colour last clicked: edit, move left or
//!   right, delete, and "New colour".

use xarast_app::colour_bar::{ColourBarOp, ColourSource, DragPoint, Swatch};
use xarast_app::colour_editor::{ColourEditorOp, ColourTarget, PaintSlot};
use xarast_color::{ColourId, ColourValue};

use crate::a11y;
use crate::model::{CommandSink, UiCommand, UiModel};
use crate::panel::PanelCtx;
use crate::theme::SWATCH_SIZE;

/// The bar's height, in points.
pub const COLOUR_BAR_HEIGHT: f32 = SWATCH_SIZE + 10.0;

/// The gap between two swatches, in points.
const GAP: f32 = 2.0;

/// The swatches the bar and the gallery show: the application's view when
/// a document is open, the model's plain palette otherwise.
pub fn swatches(model: &UiModel) -> Vec<Swatch> {
    match &model.colour_bar {
        Some(v) => v.swatches.clone(),
        None => model
            .palette
            .iter()
            .map(|e| Swatch {
                source: e
                    .colour
                    .map_or(ColourSource::NoColour, ColourSource::Direct),
                name: e.name.clone(),
                value: e.colour,
                named: e.named,
                parent: None,
                kind: "",
            })
            .collect(),
    }
}

/// The colour as egui paints it.
pub fn colour32(c: ColourValue) -> egui::Color32 {
    let rgba = c.to_rgba8();
    egui::Color32::from_rgba_unmultiplied(rgba.r, rgba.g, rgba.b, rgba.a)
}

/// The `#RRGGBB` form of a swatch; "none" for "no colour".
pub fn swatch_value(s: &Swatch) -> String {
    s.value.map_or_else(
        || "none".to_owned(),
        |v| {
            let c = v.to_rgba8();
            format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
        },
    )
}

/// Paints one swatch: its colour, or a hollow square crossed through for
/// "no colour" (never white: the difference matters); a named colour gets
/// an accent outline.
pub fn paint_swatch(
    painter: &egui::Painter,
    rect: egui::Rect,
    s: &Swatch,
    tokens: &crate::theme::ThemeTokens,
) {
    match s.value {
        Some(v) => {
            painter.rect_filled(rect, 1.0, colour32(v));
        }
        None => {
            painter.rect_stroke(
                rect,
                1.0,
                egui::Stroke::new(1.0_f32, tokens.border),
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [rect.left_bottom(), rect.right_top()],
                egui::Stroke::new(1.0_f32, tokens.error),
            );
        }
    }
    if s.named {
        painter.rect_stroke(
            rect,
            1.0,
            egui::Stroke::new(1.0_f32, tokens.accent),
            egui::StrokeKind::Outside,
        );
    }
}

/// Publishes a swatch to AccessKit: a button named after the colour, its
/// `#RRGGBB` as the value.
pub fn publish_swatch(r: &egui::Response, s: &Swatch) {
    let label = a11y::swatch_label(&s.name, s.named);
    let value = swatch_value(s);
    r.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone());
        info.current_text_value = Some(value.clone());
        info
    });
}

// ── the colour drag, shared by the bar and the gallery ────────────────────

/// The named colours that take a drop this frame and the last, by screen
/// rectangle. The drag's owner reads the previous frame's too: the widget
/// that owns the drag may be drawn before the one under the pointer.
#[derive(Clone, Default)]
struct Slots {
    pass: u64,
    now: Vec<(egui::Rect, ColourId)>,
    before: Vec<(egui::Rect, ColourId)>,
}

fn slots_id() -> egui::Id {
    egui::Id::new("xarast_colour_slots")
}

fn owner_id() -> egui::Id {
    egui::Id::new("xarast_colour_drag_owner")
}

/// Registers a named colour a dragged colour can be dropped on.
pub fn register_slot(ctx: &egui::Context, rect: egui::Rect, id: ColourId) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let s: &mut Slots = d.get_temp_mut_or_default(slots_id());
        if s.pass != pass {
            s.before = std::mem::take(&mut s.now);
            s.pass = pass;
        }
        s.now.push((rect, id));
    });
}

fn slot_at(ctx: &egui::Context, p: egui::Pos2) -> Option<ColourId> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| {
        let s: Slots = d.get_temp(slots_id()).unwrap_or_default();
        let current = if s.pass == pass { &s.now[..] } else { &[] };
        let previous = if s.pass == pass { &s.before } else { &s.now };
        current
            .iter()
            .chain(previous.iter())
            .find(|(r, _)| r.contains(p))
            .map(|(_, id)| *id)
    })
}

/// Drives a colour drag from a swatch's response: picks the colour up when
/// the drag starts, reports where it is every frame, drops it on release
/// and cancels it on `Esc`. The pointer shape says whether a drop here
/// would do anything.
pub fn drive_drag(
    ui: &egui::Ui,
    r: &egui::Response,
    source: ColourSource,
    model: &UiModel,
    out: &mut CommandSink,
) {
    let ctx = ui.ctx();
    // The drag's owner and whether it was cancelled, kept by us rather than
    // read from egui's drag state: egui drops a drag on `Esc` itself, and
    // the release must then drop nothing.
    if r.drag_started() {
        ctx.data_mut(|d| d.insert_temp(owner_id(), Some((r.id, false))));
        out.push(UiCommand::ColourBar(ColourBarOp::DragBegin(source)));
    }
    let Some((owner, cancelled)) = ctx
        .data(|d| d.get_temp::<Option<(egui::Id, bool)>>(owner_id()))
        .flatten()
    else {
        return;
    };
    if owner != r.id {
        return;
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        ctx.data_mut(|d| d.insert_temp::<Option<(egui::Id, bool)>>(owner_id(), None));
        if !cancelled {
            out.push(UiCommand::ColourBar(ColourBarOp::DragDrop));
        }
        return;
    }
    if cancelled {
        return;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.insert_temp(owner_id(), Some((r.id, true))));
        out.push(UiCommand::ColourBar(ColourBarOp::DragCancel));
        return;
    }
    if let Some(p) = ctx.pointer_latest_pos() {
        match slot_at(ctx, p) {
            Some(id) => out.push(UiCommand::ColourBar(ColourBarOp::DragTo(DragPoint::Entry(
                id,
            )))),
            None => out.push(UiCommand::ColourDragAt {
                x: p.x,
                y: p.y,
                shift: ui.input(|i| i.modifiers.shift),
            }),
        }
    }
    let allowed = model
        .colour_bar
        .as_ref()
        .and_then(|v| v.drag.as_ref())
        .map(xarast_app::colour_bar::ColourDragView::allowed);
    ctx.set_cursor_icon(match allowed {
        Some(true) => egui::CursorIcon::Copy,
        Some(false) => egui::CursorIcon::NoDrop,
        None => egui::CursorIcon::Grabbing,
    });
}

/// What a click on a swatch does: left = fill, right or `Shift`+left =
/// line.
pub fn click_slot(ui: &egui::Ui, r: &egui::Response) -> Option<PaintSlot> {
    if r.secondary_clicked() {
        return Some(PaintSlot::Stroke);
    }
    if r.clicked() {
        return Some(if ui.input(|i| i.modifiers.shift) {
            PaintSlot::Stroke
        } else {
            PaintSlot::Fill
        });
    }
    None
}

/// The first free "Colour n" among the swatches' names.
pub fn fresh_name(swatches: &[Swatch]) -> String {
    (1..)
        .map(|n| format!("Colour {n}"))
        .find(|n| swatches.iter().all(|s| &s.name != n))
        .unwrap_or_else(|| "Colour".to_owned())
}

// ── the bar ───────────────────────────────────────────────────────────────

/// The colour bar. What it keeps is interaction state: the first swatch
/// shown and the named colour its menu acts on.
#[derive(Debug, Default)]
pub struct ColourBar {
    first: usize,
    current: Option<ColourId>,
}

impl ColourBar {
    /// A bar scrolled to its start.
    pub fn new() -> ColourBar {
        ColourBar::default()
    }

    /// The index of the first swatch shown.
    pub fn first(&self) -> usize {
        self.first
    }

    /// Draws the bar across the available width.
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        let swatches = swatches(ctx.model);
        if let Some(c) = self.current
            && !swatches.iter().any(|s| s.source == ColourSource::Named(c))
        {
            self.current = None;
        }
        let step = SWATCH_SIZE + GAP;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let button = egui::vec2(SWATCH_SIZE, SWATCH_SIZE);
            // The two page buttons and the menu button, then the strip.
            let strip_w = (ui.available_width() - 3.0 * (button.x + 3.0 * GAP)).max(step);
            let visible = ((strip_w + GAP) / step).floor().max(1.0) as usize;
            let max_first = swatches.len().saturating_sub(visible);
            self.first = self.first.min(max_first);

            let left = ui
                .add_enabled(self.first > 0, egui::Button::new("\u{25C0}").small())
                .on_hover_text("Previous colours");
            a11y::set_label(ui.ctx(), left.id, "Previous colours");
            if left.clicked() {
                self.first = self.first.saturating_sub(visible);
            }

            let (strip, bg) =
                ui.allocate_exact_size(egui::vec2(strip_w, SWATCH_SIZE), egui::Sense::click());
            a11y::set_label(ui.ctx(), bg.id, "Colour bar");
            self.strip(ui, strip, &swatches, visible, ctx);
            if bg.hovered() {
                let notch = ui.input(|i| i.raw_scroll_delta.x + i.raw_scroll_delta.y);
                if notch < 0.0 {
                    self.first = (self.first + 3).min(max_first);
                } else if notch > 0.0 {
                    self.first = self.first.saturating_sub(3);
                }
            }
            bg.context_menu(|ui| self.menu(ui, &swatches, ctx));

            let right = ui
                .add_enabled(
                    self.first < max_first,
                    egui::Button::new("\u{25B6}").small(),
                )
                .on_hover_text("More colours");
            a11y::set_label(ui.ctx(), right.id, "More colours");
            if right.clicked() {
                self.first = (self.first + visible).min(max_first);
            }
            let menu = ui.menu_button("\u{2630}", |ui| self.menu(ui, &swatches, ctx));
            a11y::set_label(ui.ctx(), menu.response.id, "Colour menu");
        });
    }

    fn strip(
        &mut self,
        ui: &mut egui::Ui,
        strip: egui::Rect,
        swatches: &[Swatch],
        visible: usize,
        ctx: &mut PanelCtx<'_>,
    ) {
        let step = SWATCH_SIZE + GAP;
        let painter = ui.painter_at(strip.expand(2.0));
        for (k, (i, s)) in swatches
            .iter()
            .enumerate()
            .skip(self.first)
            .take(visible)
            .enumerate()
        {
            let rect = egui::Rect::from_min_size(
                egui::pos2(strip.left() + k as f32 * step, strip.top()),
                egui::vec2(SWATCH_SIZE, SWATCH_SIZE),
            );
            let r = ui.interact(
                rect,
                ui.id().with(("xarast_colour_bar_swatch", i)),
                egui::Sense::click_and_drag(),
            );
            publish_swatch(&r, s);
            paint_swatch(&painter, rect, s, ctx.tokens);
            if let ColourSource::Named(id) = s.source {
                register_slot(ui.ctx(), rect, id);
                if self.current == Some(id) {
                    painter.rect_stroke(
                        rect.expand(1.0),
                        1.0,
                        egui::Stroke::new(1.0_f32, ctx.tokens.focus),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            if r.has_focus() {
                painter.rect_stroke(
                    rect,
                    1.0,
                    egui::Stroke::new(2.0_f32, ctx.tokens.focus),
                    egui::StrokeKind::Outside,
                );
            }
            let r = r.on_hover_text(format!("{} ({})", s.name, swatch_value(s)));
            if let Some(slot) = click_slot(ui, &r) {
                if let ColourSource::Named(id) = s.source {
                    self.current = Some(id);
                }
                ctx.out.push(UiCommand::ColourBar(ColourBarOp::Apply {
                    source: s.source,
                    slot,
                }));
            }
            drive_drag(ui, &r, s.source, ctx.model, ctx.out);
        }
    }

    /// The bar's menu, acting on the named colour last clicked.
    fn menu(&mut self, ui: &mut egui::Ui, swatches: &[Swatch], ctx: &mut PanelCtx<'_>) {
        let named: Vec<&Swatch> = swatches.iter().filter(|s| s.named).collect();
        let current = self.current.and_then(|c| {
            named
                .iter()
                .position(|s| s.source == ColourSource::Named(c))
                .map(|i| (c, i, named[i].name.clone()))
        });
        if ui.button("New colour").clicked() {
            ctx.out
                .push(UiCommand::ColourEditor(ColourEditorOp::NewNamed(
                    fresh_name(swatches),
                )));
            ui.close();
        }
        let Some((id, i, name)) = current else {
            ui.label("Click a named colour to edit, move or delete it.");
            return;
        };
        ui.separator();
        if ui.button(format!("Edit \u{2018}{name}\u{2019}")).clicked() {
            ctx.out
                .push(UiCommand::ColourEditor(ColourEditorOp::SetTarget(
                    ColourTarget::Entry(id),
                )));
            ui.close();
        }
        if ui
            .add_enabled(
                i > 0,
                egui::Button::new(format!("Move \u{2018}{name}\u{2019} left")),
            )
            .clicked()
        {
            ctx.out.push(UiCommand::ColourBar(ColourBarOp::MoveEntry {
                id,
                to: i - 1,
            }));
            ui.close();
        }
        if ui
            .add_enabled(
                i + 1 < named.len(),
                egui::Button::new(format!("Move \u{2018}{name}\u{2019} right")),
            )
            .clicked()
        {
            ctx.out.push(UiCommand::ColourBar(ColourBarOp::MoveEntry {
                id,
                to: i + 1,
            }));
            ui.close();
        }
        if ui
            .button(format!("Delete \u{2018}{name}\u{2019}"))
            .clicked()
        {
            ctx.out.push(UiCommand::ColourBar(ColourBarOp::Delete(id)));
            self.current = None;
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PaletteEntry;
    use crate::theme::{ResolvedTheme, ThemeTokens};

    fn model(n: usize) -> UiModel {
        let mut palette = vec![PaletteEntry::none()];
        for i in 0..n {
            let t = i as f32 / n.max(1) as f32;
            palette.push(PaletteEntry::colour(
                format!("Swatch {i}"),
                ColourValue::rgb(t, 1.0 - t, 0.5),
            ));
        }
        UiModel {
            palette,
            ..Default::default()
        }
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, COLOUR_BAR_HEIGHT),
            )),
            events,
            ..Default::default()
        }
    }

    fn frames(bar: &mut ColourBar, m: &UiModel, inputs: Vec<egui::RawInput>) -> Vec<UiCommand> {
        let tokens = ThemeTokens::of(ResolvedTheme::Dark);
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        for i in inputs {
            let mut sink = CommandSink::new();
            let _ = ctx.run(i, |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    let mut pctx = PanelCtx {
                        model: m,
                        tokens: &tokens,
                        out: &mut sink,
                    };
                    bar.ui(ui, &mut pctx);
                });
            });
            out.extend(sink.drain());
        }
        out
    }

    /// The centre of the `k`-th visible swatch, found through AccessKit
    /// rather than by assuming the layout.
    fn swatch_centre(bar: &mut ColourBar, m: &UiModel, label: &str) -> egui::Pos2 {
        let tokens = ThemeTokens::of(ResolvedTheme::Dark);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut sink = CommandSink::new();
        let out = ctx.run(input(vec![]), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                let mut pctx = PanelCtx {
                    model: m,
                    tokens: &tokens,
                    out: &mut sink,
                };
                bar.ui(ui, &mut pctx);
            });
        });
        let update = out.platform_output.accesskit_update.expect("a tree");
        update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(label))
            .and_then(|(_, n)| n.bounds())
            .map(|b| egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
            .expect("the swatch is published")
    }

    fn press(p: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn a_long_palette_shows_a_page_and_pages_on() {
        let m = model(500);
        let mut bar = ColourBar::new();
        let out = frames(&mut bar, &m, vec![input(vec![]), input(vec![])]);
        assert!(out.is_empty(), "{out:?}");
        assert_eq!(bar.first(), 0);
        let more = swatch_centre(&mut bar, &m, "More colours");
        let out = frames(
            &mut bar,
            &m,
            vec![
                input(vec![egui::Event::PointerMoved(more)]),
                input(vec![press(more, egui::PointerButton::Primary, true)]),
                input(vec![press(more, egui::PointerButton::Primary, false)]),
            ],
        );
        assert!(out.is_empty(), "{out:?}");
        assert!(bar.first() > 5, "paged to {}", bar.first());
    }

    #[test]
    fn left_click_fills_and_right_click_sets_the_line() {
        let m = model(4);
        let mut bar = ColourBar::new();
        let at = swatch_centre(&mut bar, &m, "Swatch 1");
        let out = frames(
            &mut bar,
            &m,
            vec![
                input(vec![egui::Event::PointerMoved(at)]),
                input(vec![press(at, egui::PointerButton::Primary, true)]),
                input(vec![press(at, egui::PointerButton::Primary, false)]),
                input(vec![press(at, egui::PointerButton::Secondary, true)]),
                input(vec![press(at, egui::PointerButton::Secondary, false)]),
            ],
        );
        let slots: Vec<PaintSlot> = out
            .iter()
            .filter_map(|c| match c {
                UiCommand::ColourBar(ColourBarOp::Apply { slot, source }) => {
                    assert!(matches!(source, ColourSource::Direct(_)));
                    Some(*slot)
                }
                _ => None,
            })
            .collect();
        assert_eq!(slots, vec![PaintSlot::Fill, PaintSlot::Stroke]);
    }

    #[test]
    fn a_drag_picks_the_colour_up_reports_where_it_is_and_drops_it() {
        let m = model(4);
        let mut bar = ColourBar::new();
        let at = swatch_centre(&mut bar, &m, "No colour");
        let far = egui::pos2(300.0, 5.0);
        let out = frames(
            &mut bar,
            &m,
            vec![
                input(vec![egui::Event::PointerMoved(at)]),
                input(vec![press(at, egui::PointerButton::Primary, true)]),
                input(vec![egui::Event::PointerMoved(at + egui::vec2(20.0, 0.0))]),
                input(vec![egui::Event::PointerMoved(far)]),
                input(vec![press(far, egui::PointerButton::Primary, false)]),
                input(vec![]),
            ],
        );
        assert!(
            out.contains(&UiCommand::ColourBar(ColourBarOp::DragBegin(
                ColourSource::NoColour
            ))),
            "{out:?}"
        );
        assert!(out.iter().any(|c| matches!(
            c,
            UiCommand::ColourDragAt { x, .. } if (*x - 300.0).abs() < 0.5
        )));
        assert_eq!(
            out.last(),
            Some(&UiCommand::ColourBar(ColourBarOp::DragDrop))
        );
        assert!(
            !out.iter()
                .any(|c| matches!(c, UiCommand::ColourBar(ColourBarOp::Apply { .. }))),
            "a drag is not a click"
        );
    }

    #[test]
    fn escape_cancels_the_drag_and_the_release_drops_nothing() {
        let m = model(4);
        let mut bar = ColourBar::new();
        let at = swatch_centre(&mut bar, &m, "Swatch 0");
        let far = egui::pos2(300.0, 5.0);
        let esc = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let out = frames(
            &mut bar,
            &m,
            vec![
                input(vec![egui::Event::PointerMoved(at)]),
                input(vec![press(at, egui::PointerButton::Primary, true)]),
                input(vec![egui::Event::PointerMoved(far)]),
                input(vec![esc]),
                input(vec![egui::Event::PointerMoved(far + egui::vec2(-5.0, 0.0))]),
                input(vec![press(far, egui::PointerButton::Primary, false)]),
                input(vec![]),
            ],
        );
        assert!(
            out.contains(&UiCommand::ColourBar(ColourBarOp::DragCancel)),
            "{out:?}"
        );
        assert!(!out.contains(&UiCommand::ColourBar(ColourBarOp::DragDrop)));
    }

    #[test]
    fn no_colour_is_hollow_and_every_swatch_has_a_value() {
        let s = &swatches(&model(1))[0];
        assert_eq!(s.source, ColourSource::NoColour);
        assert_eq!(swatch_value(s), "none");
        let red = Swatch {
            source: ColourSource::Direct(ColourValue::rgb(1.0, 0.0, 0.0)),
            name: "Red".to_owned(),
            value: Some(ColourValue::rgb(1.0, 0.0, 0.0)),
            named: false,
            parent: None,
            kind: "",
        };
        assert_eq!(swatch_value(&red), "#FF0000");
        assert_eq!(fresh_name(&[red]), "Colour 1");
    }
}
