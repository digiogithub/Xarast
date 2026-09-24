//! The photo panel (phase 10, T10.6.8): the non-destructive adjustments of
//! the one selected bitmap object.
//!
//! It draws [`xarast_app::photo_panel::PhotoPanelView`] and answers with
//! [`UiCommand::PhotoPanel`]; the chain, the live preview at proxy
//! resolution and the undo steps are `xarast-app`'s. From top to bottom:
//! the bitmap's name and size, the chain as a list, the tone sliders
//! (brightness, contrast, gamma, saturation) and greyscale, levels for a
//! chosen channel, the orientation buttons, the crop in master pixels, and
//! "Reset all". A chain holding an operation this version does not know is
//! listed and nothing else: it cannot be edited.
//!
//! A slider **dragged** sends `Preview` every frame it moves and `Commit`
//! on release, so the drag is one undo step; `Esc` while it is held sends
//! `Cancel` and the rest of that drag is ignored. A slider moved with the
//! keyboard, or a value typed, a button or a checkbox, sends `Set`: one
//! step each. The crop fields never preview (a crop moves the object); a
//! dragged crop field is applied on release.

use xarast_app::photo_panel::{
    GAMMA_RANGE, Levels, LevelsChannel, PhotoOp, PhotoOps, PhotoOrient, PhotoPanelOp,
    PhotoPanelView, PixelRect, channel_label, op_label,
};

use crate::a11y;
use crate::model::UiCommand;
use crate::panel::{Panel, PanelCtx, PanelId};

/// The photo panel's identifier.
pub const ID: PanelId = PanelId("photo");

/// The channels the levels chooser offers, in order.
pub const LEVELS_CHANNELS: [LevelsChannel; 4] = [
    LevelsChannel::All,
    LevelsChannel::Red,
    LevelsChannel::Green,
    LevelsChannel::Blue,
];

/// The photo panel. It keeps interaction state only: the drag cancelled by
/// `Esc`, the levels channel shown, a crop field being dragged.
#[derive(Debug)]
pub struct PhotoPanel {
    cancelled_drag: Option<egui::Id>,
    channel: LevelsChannel,
    /// A crop dragged in a field, applied on release: `[x, y, w, h]`.
    crop_drag: Option<[u32; 4]>,
}

impl Default for PhotoPanel {
    fn default() -> Self {
        PhotoPanel::new()
    }
}

impl PhotoPanel {
    /// A fresh panel.
    pub fn new() -> PhotoPanel {
        PhotoPanel {
            cancelled_drag: None,
            channel: LevelsChannel::All,
            crop_drag: None,
        }
    }
}

/// How a slider was used this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    None,
    Live,
    Set,
    Commit,
    /// The release frame also moved the value: preview it, then commit.
    LiveCommit,
    Cancel,
}

impl Panel for PhotoPanel {
    fn id(&self) -> PanelId {
        ID
    }

    fn title(&self) -> &str {
        "Photo"
    }

    fn min_size(&self) -> egui::Vec2 {
        egui::vec2(240.0, 160.0)
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) {
        let Some(view) = ctx.model.photo_panel.clone() else {
            ui.label("Open a document to adjust its photos.");
            return;
        };
        if view.node.is_none() {
            ui.label(match view.selected {
                0 => "Select a placed bitmap to adjust it.".to_owned(),
                n => format!(
                    "{n} {} selected: select one placed bitmap to adjust it.",
                    if n == 1 { "object" } else { "objects" }
                ),
            });
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("xarast_photo_panel")
            .auto_shrink([false, true])
            .show(ui, |ui| self.editor(ui, &view, ctx));
    }
}

fn emit(ctx: &mut PanelCtx<'_>, op: PhotoPanelOp) {
    ctx.out.push(UiCommand::PhotoPanel(op));
}

impl PhotoPanel {
    /// Classifies a slider's use, remembering an `Esc` during its drag so
    /// the rest of that drag is ignored.
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
            // egui ends a drag itself when `Esc` is pressed: that release
            // is a cancel, not a commit.
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                return Use::Cancel;
            }
            return if r.changed() {
                Use::LiveCommit
            } else {
                Use::Commit
            };
        }
        if r.changed() { Use::Set } else { Use::None }
    }

    fn send(&mut self, ctx: &mut PanelCtx<'_>, how: Use, ops: PhotoOps) {
        match how {
            Use::None => {}
            Use::Live => emit(ctx, PhotoPanelOp::Preview(ops)),
            Use::Set => emit(ctx, PhotoPanelOp::Set(ops)),
            Use::Commit => emit(ctx, PhotoPanelOp::Commit),
            Use::LiveCommit => {
                emit(ctx, PhotoPanelOp::Preview(ops));
                emit(ctx, PhotoPanelOp::Commit);
            }
            Use::Cancel => emit(ctx, PhotoPanelOp::Cancel),
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui, v: &PhotoPanelView, ctx: &mut PanelCtx<'_>) {
        let title = match v.master {
            Some((w, h)) => format!("{}: {w} × {h} px", v.name),
            None => v.name.clone(),
        };
        ui.label(egui::RichText::new(title).color(ctx.tokens.text));
        chain_list(ui, v);
        if !v.editable {
            let kinds = v
                .unknown_kinds()
                .iter()
                .map(|k| format!("\u{2018}{k}\u{2019}"))
                .collect::<Vec<_>>()
                .join(", ");
            ui.label(format!(
                "Non-editable: unknown operation {kinds}. This chain comes from a newer \
                 version of Xarast; it is shown and kept, but cannot be changed here."
            ));
            return;
        }
        ui.separator();
        self.tone(ui, v, ctx);
        ui.separator();
        self.levels(ui, v, ctx);
        ui.separator();
        orientation(ui, v, ctx);
        ui.separator();
        self.crop(ui, v, ctx);
        ui.separator();
        let r = ui.add_enabled(!v.ops.is_empty(), egui::Button::new("Reset all"));
        let r = r.on_hover_text("Remove every adjustment: the original picture");
        if r.clicked() {
            emit(ctx, PhotoPanelOp::Reset);
        }
    }

    /// Brightness, contrast, gamma, saturation, greyscale.
    fn tone(&mut self, ui: &mut egui::Ui, v: &PhotoPanelView, ctx: &mut PanelCtx<'_>) {
        type Make = fn(f32) -> PhotoOp;
        let unit: [(&str, f32, Make); 3] = [
            ("Brightness", brightness(&v.ops), PhotoOp::Brightness),
            ("Contrast", contrast(&v.ops), PhotoOp::Contrast),
            ("Saturation", saturation(&v.ops), PhotoOp::Saturation),
        ];
        for (name, value, make) in unit {
            let mut pct = f64::from(value) * 100.0;
            let r = ui.add(
                egui::Slider::new(&mut pct, -100.0..=100.0)
                    .text(name)
                    .suffix(" %")
                    .fixed_decimals(0),
            );
            a11y::set_label(ui.ctx(), r.id, name);
            let how = self.classify(ui, &r);
            self.send(ctx, how, v.ops.with(make((pct / 100.0) as f32)));
        }
        let mut g = f64::from(gamma(&v.ops));
        let r = ui.add(
            egui::Slider::new(&mut g, f64::from(GAMMA_RANGE.0)..=f64::from(GAMMA_RANGE.1))
                .logarithmic(true)
                .text("Gamma")
                .fixed_decimals(2),
        );
        a11y::set_label(ui.ctx(), r.id, "Gamma");
        let how = self.classify(ui, &r);
        self.send(ctx, how, v.ops.with(PhotoOp::Gamma(g as f32)));

        let mut grey = v.ops.ops.iter().any(|o| matches!(o, PhotoOp::Greyscale));
        if ui.checkbox(&mut grey, "Greyscale").changed() {
            let ops = if grey {
                v.ops.with(PhotoOp::Greyscale)
            } else {
                PhotoOps {
                    ops: v
                        .ops
                        .ops
                        .iter()
                        .filter(|o| !matches!(o, PhotoOp::Greyscale))
                        .cloned()
                        .collect(),
                }
            };
            emit(ctx, PhotoPanelOp::Set(ops));
        }
    }

    /// Levels of the chosen channel: input and output black and white.
    fn levels(&mut self, ui: &mut egui::Ui, v: &PhotoPanelView, ctx: &mut PanelCtx<'_>) {
        let r = egui::ComboBox::from_label("Levels channel")
            .selected_text(channel_label(self.channel))
            .show_ui(ui, |ui| {
                for c in LEVELS_CHANNELS {
                    ui.selectable_value(&mut self.channel, c, channel_label(c));
                }
            });
        a11y::set_label(
            ui.ctx(),
            r.response.id,
            format!("Levels channel: {}", channel_label(self.channel)),
        );
        let current = levels_of(&v.ops, self.channel);
        let fields: [(&str, u8); 4] = [
            ("Input black", current.in_lo),
            ("Input white", current.in_hi),
            ("Output black", current.out_lo),
            ("Output white", current.out_hi),
        ];
        for (i, (name, value)) in fields.into_iter().enumerate() {
            let mut x = value;
            let r = ui.add(egui::Slider::new(&mut x, 0..=255).text(name));
            let label = format!("{name} ({})", channel_label(self.channel));
            a11y::set_label(ui.ctx(), r.id, label);
            let how = self.classify(ui, &r);
            let mut l = current;
            match i {
                0 => l.in_lo = x,
                1 => l.in_hi = x,
                2 => l.out_lo = x,
                _ => l.out_hi = x,
            }
            self.send(ctx, how, v.ops.with(PhotoOp::Levels(l)));
        }
    }

    /// The crop, in master pixels. Typed or stepped values apply at once;
    /// a dragged field applies on release (a crop moves the object, so it
    /// is never previewed).
    fn crop(&mut self, ui: &mut egui::Ui, v: &PhotoPanelView, ctx: &mut PanelCtx<'_>) {
        let Some((mw, mh)) = v.master else {
            return;
        };
        let shown = v
            .ops
            .crop()
            .and_then(|r| r.clamped(mw, mh))
            .map_or([0, 0, mw, mh], |r| [r.x, r.y, r.width, r.height]);
        let mut c = self.crop_drag.unwrap_or(shown);
        let names = ["Crop left", "Crop top", "Crop width", "Crop height"];
        let max = [mw - 1, mh - 1, mw, mh];
        let mut changed_now = None;
        ui.label("Crop (pixels of the original)");
        egui::Grid::new("xarast_photo_crop")
            .num_columns(2)
            .show(ui, |ui| {
                for i in 0..4 {
                    ui.label(&names[i][5..]);
                    let min = u32::from(i >= 2);
                    let r = ui.add(egui::DragValue::new(&mut c[i]).range(min..=max[i]));
                    a11y::set_label(ui.ctx(), r.id, names[i]);
                    if r.dragged() {
                        self.crop_drag = Some(c);
                    } else if r.drag_stopped() {
                        changed_now = Some(c);
                        self.crop_drag = None;
                    } else if r.changed() {
                        changed_now = Some(c);
                    }
                    ui.end_row();
                }
            });
        if let Some([x, y, w, h]) = changed_now {
            let rect = PixelRect {
                x,
                y,
                width: w.min(mw.saturating_sub(x)).max(1),
                height: h.min(mh.saturating_sub(y)).max(1),
            };
            let whole = rect.x == 0 && rect.y == 0 && rect.width == mw && rect.height == mh;
            emit(
                ctx,
                PhotoPanelOp::Set(without_crop(&v.ops, (!whole).then_some(rect))),
            );
        }
        let r = ui.add_enabled(v.ops.crop().is_some(), egui::Button::new("Remove crop"));
        if r.clicked() {
            emit(ctx, PhotoPanelOp::Set(without_crop(&v.ops, None)));
        }
    }
}

/// The chain, one list item per operation.
fn chain_list(ui: &mut egui::Ui, v: &PhotoPanelView) {
    if v.ops.is_empty() {
        ui.label("No adjustments: the original picture.");
        return;
    }
    let count = v.ops.ops.len();
    ui.label("Adjustments, in the order they apply:");
    for (i, op) in v.ops.ops.iter().enumerate() {
        let text = op_label(op);
        let r = ui.label(format!("{}. {text}", i + 1));
        a11y::set_list_item(ui.ctx(), r.id, i, count);
        a11y::set_label(ui.ctx(), r.id, text);
    }
}

/// Rotate and flip buttons: each one step, composed with the orientation
/// the chain already has.
fn orientation(ui: &mut egui::Ui, v: &PhotoPanelView, ctx: &mut PanelCtx<'_>) {
    let ccw = PhotoOrient {
        turns: 3,
        flip: false,
    };
    let buttons: [(&str, &str, PhotoOrient); 4] = [
        ("Rotate left", "Turn a quarter turn anticlockwise", ccw),
        (
            "Rotate right",
            "Turn a quarter turn clockwise",
            PhotoOrient::CW,
        ),
        (
            "Flip horizontal",
            "Mirror left to right",
            PhotoOrient::FLIP_H,
        ),
        ("Flip vertical", "Mirror top to bottom", PhotoOrient::FLIP_V),
    ];
    ui.horizontal_wrapped(|ui| {
        for (name, tip, o) in buttons {
            if ui.button(name).on_hover_text(tip).clicked() {
                let next = v.ops.orient().then(o);
                emit(ctx, PhotoPanelOp::Set(v.ops.with(PhotoOp::Orient(next))));
            }
        }
    });
}

fn without_crop(ops: &PhotoOps, crop: Option<PixelRect>) -> PhotoOps {
    let mut out = PhotoOps {
        ops: ops
            .ops
            .iter()
            .filter(|o| !matches!(o, PhotoOp::Crop(_)))
            .cloned()
            .collect(),
    };
    if let Some(r) = crop {
        out = out.with(PhotoOp::Crop(r));
    }
    out
}

fn find(ops: &PhotoOps, f: impl Fn(&PhotoOp) -> Option<f32>) -> Option<f32> {
    ops.ops.iter().rev().find_map(f)
}

/// The chain's brightness (0 when none).
pub fn brightness(ops: &PhotoOps) -> f32 {
    find(ops, |o| match o {
        PhotoOp::Brightness(v) => Some(*v),
        _ => None,
    })
    .unwrap_or(0.0)
}

/// The chain's contrast (0 when none).
pub fn contrast(ops: &PhotoOps) -> f32 {
    find(ops, |o| match o {
        PhotoOp::Contrast(v) => Some(*v),
        _ => None,
    })
    .unwrap_or(0.0)
}

/// The chain's saturation change (0 when none).
pub fn saturation(ops: &PhotoOps) -> f32 {
    find(ops, |o| match o {
        PhotoOp::Saturation(v) => Some(*v),
        _ => None,
    })
    .unwrap_or(0.0)
}

/// The chain's gamma (1 when none).
pub fn gamma(ops: &PhotoOps) -> f32 {
    find(ops, |o| match o {
        PhotoOp::Gamma(v) => Some(*v),
        _ => None,
    })
    .unwrap_or(1.0)
}

/// The chain's levels for `channel` (the identity when none).
pub fn levels_of(ops: &PhotoOps, channel: LevelsChannel) -> Levels {
    ops.ops
        .iter()
        .rev()
        .find_map(|o| match o {
            PhotoOp::Levels(l) if l.channel == channel => Some(*l),
            _ => None,
        })
        .unwrap_or(Levels {
            channel,
            in_lo: 0,
            in_hi: 255,
            out_lo: 0,
            out_hi: 255,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_default_to_neutral_and_crop_edits_keep_the_rest() {
        let ops = PhotoOps::new();
        assert_eq!(
            (
                brightness(&ops),
                contrast(&ops),
                saturation(&ops),
                gamma(&ops)
            ),
            (0.0, 0.0, 0.0, 1.0)
        );
        assert!(levels_of(&ops, LevelsChannel::Red).is_identity());
        let ops = PhotoOps {
            ops: vec![
                PhotoOp::Crop(PixelRect {
                    x: 1,
                    y: 1,
                    width: 5,
                    height: 5,
                }),
                PhotoOp::Brightness(0.2),
            ],
        };
        let cleared = without_crop(&ops, None);
        assert_eq!(cleared.ops, [PhotoOp::Brightness(0.2)]);
        let r = PixelRect {
            x: 2,
            y: 3,
            width: 4,
            height: 4,
        };
        assert_eq!(without_crop(&ops, Some(r)).crop(), Some(r));
    }
}
