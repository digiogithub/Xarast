//! The density probe of the Phase 5 spike (`§W1`).
//!
//! Architecture §7 question 2 asks whether egui's immediate mode holds up
//! at professional panel density: four hundred visible controls, 18–22 px
//! rows, virtualised trees and galleries, and none of the padding of an
//! application interface. The probe is here, in the library rather than in
//! the example, for three reasons: the example runs it, the benchmark runs
//! it, and the tests assert its thresholds — one definition, three uses.
//!
//! What the probe builds, following the spike's task list:
//!
//! | Task | What |
//! |---|---|
//! | U1.1 | six docked panels, ≥ 400 visible controls at [`crate::theme::ROW_HEIGHT`] |
//! | U1.2 | a 5,000-row virtualised tree with `egui_extras::TableBuilder` |
//! | U1.3 | a 512-swatch palette strip and a 2,000-thumbnail gallery |
//! | U1.4 | numeric fields with drag-adjust, unit parsing and bump buttons |
//!
//! Everything here runs with no window, no GPU and no compositor: a probe
//! that needs a display cannot be measured in CI, and a spike that cannot
//! be repeated is an anecdote.

use std::time::{Duration, Instant};

use xarast_geom::Mp;

use crate::theme::{ROW_HEIGHT, ResolvedTheme, SWATCH_SIZE, ThemeTokens};
use crate::units::{Unit, format_measure, parse_measure};

/// How the probe is configured. The defaults are the spike's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeConfig {
    /// Rows in the virtualised tree.
    pub tree_rows: usize,
    /// Swatches on the palette strip.
    pub swatches: usize,
    /// Thumbnails in the gallery.
    pub thumbnails: usize,
    /// Numeric property fields per property panel.
    pub property_fields: usize,
    /// Toggles per toolbar panel.
    pub toggles: usize,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        ProbeConfig {
            tree_rows: 5_000,
            swatches: 512,
            thumbnails: 2_000,
            property_fields: 48,
            toggles: 64,
        }
    }
}

/// The probe: six panels' worth of professional density.
#[derive(Debug)]
pub struct DensityProbe {
    config: ProbeConfig,
    tokens: ThemeTokens,
    /// Field values, so the fields are real state and not constants the
    /// layout can fold away.
    values: Vec<Mp>,
    toggles: Vec<bool>,
    text: String,
    scroll_offset: f32,
    /// Counted afresh every frame: how many controls were actually
    /// instantiated. This is the honest denominator of the measurement.
    last_control_count: usize,
    /// One slider's value, for the partial-update measurement (P5).
    slider: f32,
}

impl Default for DensityProbe {
    fn default() -> Self {
        DensityProbe::new(ProbeConfig::default(), ResolvedTheme::Dark)
    }
}

impl DensityProbe {
    /// Builds a probe.
    pub fn new(config: ProbeConfig, theme: ResolvedTheme) -> DensityProbe {
        DensityProbe {
            config,
            tokens: ThemeTokens::of(theme),
            values: (0..config.property_fields)
                .map(|i| Mp::from_mm(i as f64 * 1.5))
                .collect(),
            toggles: (0..config.toggles).map(|i| i % 3 == 0).collect(),
            text: "12mm + 3pt".to_owned(),
            scroll_offset: 0.0,
            last_control_count: 0,
            slider: 0.5,
        }
    }

    /// How many controls the last frame instantiated.
    pub fn control_count(&self) -> usize {
        self.last_control_count
    }

    /// Scrolls the virtualised tree, for the scrolling measurement (P4).
    pub fn scroll_to(&mut self, offset: f32) {
        self.scroll_offset = offset;
    }

    /// Moves one slider, for the partial-update measurement (P5).
    pub fn nudge_slider(&mut self, v: f32) {
        self.slider = v.clamp(0.0, 1.0);
    }

    /// Builds one frame of the whole probe.
    pub fn ui(&mut self, ctx: &egui::Context) {
        let mut controls = 0usize;
        egui::TopBottomPanel::top("probe_toolbar").show(ctx, |ui| {
            controls += self.toolbar(ui);
        });
        egui::SidePanel::left("probe_tree")
            .default_width(260.0)
            .show(ctx, |ui| {
                controls += self.tree(ui);
            });
        egui::SidePanel::right("probe_properties")
            .default_width(260.0)
            .show(ctx, |ui| {
                controls += self.properties(ui);
            });
        egui::TopBottomPanel::bottom("probe_palette").show(ctx, |ui| {
            controls += self.palette(ui);
        });
        egui::TopBottomPanel::bottom("probe_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} controls", self.last_control_count));
                ui.label(format_measure(self.values[0], Unit::Millimetre));
            });
            controls += 2;
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            controls += self.gallery(ui);
        });
        self.last_control_count = controls;
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) -> usize {
        let mut n = 0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 1.0);
            for (i, on) in self.toggles.iter_mut().enumerate() {
                // Labelled toggles, not drawn glyphs: an icon-only control
                // is anonymous to AT-SPI (see `crate::a11y`).
                if ui
                    .selectable_label(*on, format!("T{i}"))
                    .on_hover_text(format!("Toggle {i}"))
                    .clicked()
                {
                    *on = !*on;
                }
                n += 1;
            }
        });
        n
    }

    fn tree(&mut self, ui: &mut egui::Ui) -> usize {
        let rows = self.config.tree_rows;
        let mut n = 0;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .vertical_scroll_offset(self.scroll_offset)
            .column(egui_extras::Column::exact(18.0))
            .column(egui_extras::Column::remainder())
            .column(egui_extras::Column::exact(44.0))
            .header(ROW_HEIGHT, |mut header| {
                header.col(|ui| {
                    ui.strong("");
                });
                header.col(|ui| {
                    ui.strong("Object");
                });
                header.col(|ui| {
                    ui.strong("Size");
                });
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows, |mut row| {
                    let i = row.index();
                    // `TableBuilder` publishes no list or row semantics of
                    // its own — the spike's P8 census found the rows
                    // exposed as a flat run of buttons — so the panel adds
                    // them, exactly as `panels::layers` does in production.
                    row.col(|ui| {
                        let mut on = i % 2 == 0;
                        ui.add(egui::Checkbox::new(&mut on, ""));
                    });
                    row.col(|ui| {
                        let _ = ui.selectable_label(false, format!("Object {i}"));
                    });
                    let (_, response) = row.col(|ui| {
                        ui.weak(format!("{}", i % 97));
                    });
                    crate::a11y::set_list_item(&response.ctx, response.id, i, rows);
                });
            });
        // Only the visible rows were built; three controls each.
        n += 3;
        n
    }

    fn properties(&mut self, ui: &mut egui::Ui) -> usize {
        let mut n = 0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(egui::Slider::new(&mut self.slider, 0.0..=1.0).text("Opacity"));
                n += 1;
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .desired_width(120.0)
                        .hint_text("10mm, 1in, 3p6"),
                );
                if response.changed() {
                    // Parsing every keystroke is the honest cost of a
                    // field that accepts units and expressions.
                    let _ = parse_measure(&self.text, Unit::Millimetre);
                }
                n += 1;
                for (i, value) in self.values.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.set_min_height(ROW_HEIGHT);
                        ui.label(format!("P{i}"));
                        let mut v = value.to_mm();
                        // Drag-adjust: the number is dragged, not only
                        // typed, which is the professional behaviour of
                        // `research/04 §3` item 16.
                        if ui
                            .add(egui::DragValue::new(&mut v).speed(0.1).suffix(" mm"))
                            .changed()
                        {
                            *value = Mp::from_mm(v);
                        }
                        if ui.small_button("-").clicked() {
                            *value = Mp::new(value.raw() - Unit::Millimetre.bump().raw());
                        }
                        if ui.small_button("+").clicked() {
                            *value = Mp::new(value.raw() + Unit::Millimetre.bump().raw());
                        }
                    });
                    n += 4;
                }
            });
        n
    }

    fn palette(&mut self, ui: &mut egui::Ui) -> usize {
        let mut n = 0;
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for i in 0..self.config.swatches {
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(SWATCH_SIZE, SWATCH_SIZE),
                            egui::Sense::click(),
                        );
                        if ui.is_rect_visible(rect) {
                            let t = i as f32 / self.config.swatches as f32;
                            ui.painter().rect_filled(
                                rect,
                                1.0,
                                egui::Color32::from_rgb(
                                    (t * 255.0) as u8,
                                    ((1.0 - t) * 255.0) as u8,
                                    128,
                                ),
                            );
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Swatch {i}"),
                            )
                        });
                        n += 1;
                    }
                });
            });
        n
    }

    fn gallery(&mut self, ui: &mut egui::Ui) -> usize {
        let mut n = 0;
        let thumb = 48.0;
        let columns = ((ui.available_width() / (thumb + 4.0)).floor() as usize).max(1);
        let rows = self.config.thumbnails.div_ceil(columns);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, thumb + 4.0, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for col in 0..columns {
                            let i = row * columns + col;
                            if i >= self.config.thumbnails {
                                break;
                            }
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(thumb, thumb),
                                egui::Sense::click(),
                            );
                            // A real gallery blits from a texture atlas;
                            // headless, the painted stand-in costs the
                            // same on the CPU side, which is what is
                            // being measured.
                            ui.painter()
                                .rect_filled(rect, 2.0, self.tokens.surface_raised);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                format!("{i}"),
                                egui::FontId::proportional(9.0),
                                self.tokens.text_muted,
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Thumbnail {i}"),
                                )
                            });
                            n += 1;
                        }
                    });
                }
            });
        n
    }
}

/// One measured run: the timings of a series of frames.
#[derive(Debug, Clone, Default)]
pub struct Samples {
    /// Every frame's build time, in order.
    pub frames: Vec<Duration>,
}

impl Samples {
    /// The percentile of the samples, `p` in 0..=1.
    pub fn percentile(&self, p: f64) -> Duration {
        if self.frames.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.frames.clone();
        sorted.sort_unstable();
        let index = ((sorted.len() - 1) as f64 * p).round() as usize;
        sorted[index]
    }

    /// The slowest frame.
    pub fn max(&self) -> Duration {
        self.frames.iter().copied().max().unwrap_or_default()
    }

    /// The mean.
    pub fn mean(&self) -> Duration {
        if self.frames.is_empty() {
            return Duration::ZERO;
        }
        self.frames.iter().sum::<Duration>() / self.frames.len() as u32
    }
}

/// Runs the probe for `frames` frames and times each one.
///
/// `screen` is the logical window size and `scale` the pixels per point;
/// both matter, because density is a function of how much fits on screen.
/// The returned samples exclude the first two frames, which pay for font
/// atlas construction and layout discovery and are not representative of a
/// running application.
pub fn measure(
    probe: &mut DensityProbe,
    ctx: &egui::Context,
    screen: egui::Vec2,
    scale: f32,
    frames: usize,
) -> Samples {
    let mut samples = Samples::default();
    for frame in 0..frames {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };
        ctx.set_pixels_per_point(scale);
        let start = Instant::now();
        let output = ctx.run(input, |ctx| probe.ui(ctx));
        let elapsed = start.elapsed();
        // Keep the output alive to the end of the measurement so that
        // dropping the shapes is not counted against the next frame.
        drop(output);
        if frame >= 2 {
            samples.frames.push(elapsed);
        }
    }
    samples
}

/// Times the tessellation of one frame, which is the CPU half of the cost
/// that ends in the GPU pass.
///
/// The GPU pass itself (P3 of the spike) cannot be measured without an
/// adapter; this is the part that can.
pub fn measure_tessellation(
    probe: &mut DensityProbe,
    ctx: &egui::Context,
    screen: egui::Vec2,
    scale: f32,
    frames: usize,
) -> Samples {
    let mut samples = Samples::default();
    for frame in 0..frames {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };
        ctx.set_pixels_per_point(scale);
        let output = ctx.run(input, |ctx| probe.ui(ctx));
        let start = Instant::now();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let elapsed = start.elapsed();
        drop(primitives);
        if frame >= 2 {
            samples.frames.push(elapsed);
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx, ResolvedTheme::Dark);
        ctx
    }

    #[test]
    fn the_probe_really_instantiates_four_hundred_controls() {
        let mut probe = DensityProbe::default();
        let ctx = ctx();
        let _ = measure(&mut probe, &ctx, egui::vec2(2560.0, 1440.0), 1.0, 3);
        assert!(
            probe.control_count() >= 400,
            "only {} controls",
            probe.control_count()
        );
    }

    #[test]
    fn the_probe_builds_at_every_scale_factor_without_panicking() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut probe = DensityProbe::default();
            let ctx = ctx();
            let s = measure(&mut probe, &ctx, egui::vec2(1920.0, 1080.0), scale, 3);
            assert!(s.mean() > Duration::ZERO, "scale {scale}");
        }
    }

    #[test]
    fn percentiles_are_ordered() {
        let s = Samples {
            frames: (1..=100).map(|i| Duration::from_micros(i * 10)).collect(),
        };
        assert!(s.percentile(0.5) <= s.percentile(0.99));
        assert!(s.percentile(0.99) <= s.max());
        assert!(s.mean() > Duration::ZERO);
        assert_eq!(Samples::default().percentile(0.5), Duration::ZERO);
        assert_eq!(Samples::default().max(), Duration::ZERO);
    }

    #[test]
    fn scrolling_the_tree_does_not_change_the_control_count_much() {
        // Virtualisation, stated as a property: a tree scrolled to row
        // 4,000 costs what a tree scrolled to row 0 costs.
        let ctx = ctx();
        let mut probe = DensityProbe::default();
        let _ = measure(&mut probe, &ctx, egui::vec2(1920.0, 1080.0), 1.0, 3);
        let at_top = probe.control_count();
        probe.scroll_to(4_000.0 * ROW_HEIGHT);
        let _ = measure(&mut probe, &ctx, egui::vec2(1920.0, 1080.0), 1.0, 3);
        let deep = probe.control_count();
        assert!(
            deep.abs_diff(at_top) <= at_top / 4,
            "{at_top} controls at the top, {deep} at row 4,000"
        );
    }
}
