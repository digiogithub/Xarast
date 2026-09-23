//! Dark and light themes, and the density they impose.
//!
//! Two jobs live here. The first is colour: a small token set, defined once
//! per scheme, from which the egui visuals are derived — no panel ever names
//! a colour literal. The second is **density**, which is the reason the
//! theme is applied at all: a professional tool shows four hundred controls
//! at once, and egui's stock spacing is sized for an application, not a
//! tool. Every spacing number below is deliberate, and the density spike of
//! `docs/phases/phase-05-shell-and-ui.md` §W1 measures the result.
//!
//! No artwork from any other program is used, here or anywhere else in this
//! crate: the few glyphs the interface needs are drawn with the painter.

/// The system's preferred colour scheme, as reported by the shell from
/// `org.freedesktop.appearance color-scheme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    /// The portal reported "no preference", or there is no portal at all.
    #[default]
    NoPreference,
    /// The portal reported a preference for dark.
    Dark,
    /// The portal reported a preference for light.
    Light,
}

/// The theme the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Always dark, whatever the desktop says.
    Dark,
    /// Always light, whatever the desktop says.
    Light,
    /// Follow the desktop, and change live when it changes.
    #[default]
    FollowSystem,
}

impl Theme {
    /// Resolves a preference plus a system scheme into a concrete scheme.
    ///
    /// "No preference" resolves to dark: a drawing surface is judged against
    /// its surroundings, and a dark shell around the page is the convention
    /// every comparable tool follows.
    pub fn resolve(self, system: ColorScheme) -> ResolvedTheme {
        match (self, system) {
            (Theme::Dark, _) => ResolvedTheme::Dark,
            (Theme::Light, _) => ResolvedTheme::Light,
            (Theme::FollowSystem, ColorScheme::Light) => ResolvedTheme::Light,
            (Theme::FollowSystem, _) => ResolvedTheme::Dark,
        }
    }

    /// The name shown in the theme menu.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::FollowSystem => "Follow system",
        }
    }
}

/// A theme with the "follow system" indirection already resolved away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedTheme {
    /// The dark scheme.
    Dark,
    /// The light scheme.
    Light,
}

/// The colour tokens of one scheme.
///
/// Panels name tokens, never literals, so that a scheme is one edit and a
/// third scheme (high contrast, for example) is one more value of
/// [`ResolvedTheme`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeTokens {
    /// The window and panel background.
    pub surface: egui::Color32,
    /// A raised surface: the ruler strip, a table header, a toolbar.
    pub surface_raised: egui::Color32,
    /// A sunken surface: a text field, a list body.
    pub surface_sunken: egui::Color32,
    /// The area around the page, behind the document.
    pub canvas_backdrop: egui::Color32,
    /// The page itself, when the document does not say otherwise.
    pub page: egui::Color32,
    /// Primary text.
    pub text: egui::Color32,
    /// Secondary text: units, hints, disabled rows.
    pub text_muted: egui::Color32,
    /// Separators and panel borders.
    pub border: egui::Color32,
    /// The accent used for selection, focus and the active layer.
    pub accent: egui::Color32,
    /// Text drawn on top of the accent.
    pub on_accent: egui::Color32,
    /// The keyboard focus ring. Distinct from the accent on purpose:
    /// focus and selection are different states and must look different.
    pub focus: egui::Color32,
    /// Guides dragged out of the rulers.
    pub guide: egui::Color32,
    /// The grid.
    pub grid: egui::Color32,
    /// A warning, in the diagnostics list and the status bar.
    pub warning: egui::Color32,
    /// An error, in the diagnostics list and the status bar.
    pub error: egui::Color32,
}

impl ThemeTokens {
    /// The tokens of a resolved scheme.
    pub fn of(theme: ResolvedTheme) -> ThemeTokens {
        match theme {
            ResolvedTheme::Dark => DARK,
            ResolvedTheme::Light => LIGHT,
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// The dark scheme.
const DARK: ThemeTokens = ThemeTokens {
    surface: rgb(0x22, 0x24, 0x27),
    surface_raised: rgb(0x2b, 0x2e, 0x32),
    surface_sunken: rgb(0x18, 0x1a, 0x1c),
    canvas_backdrop: rgb(0x14, 0x15, 0x17),
    page: rgb(0xff, 0xff, 0xff),
    text: rgb(0xe8, 0xea, 0xed),
    text_muted: rgb(0xa0, 0xa6, 0xad),
    border: rgb(0x3a, 0x3e, 0x44),
    accent: rgb(0x3d, 0x8b, 0xfd),
    on_accent: rgb(0x0b, 0x0c, 0x0e),
    focus: rgb(0xff, 0xc4, 0x4d),
    guide: rgb(0x4f, 0xc3, 0xf7),
    grid: rgb(0x3a, 0x3e, 0x44),
    warning: rgb(0xf5, 0xb3, 0x42),
    error: rgb(0xf2, 0x6d, 0x6d),
};

/// The light scheme.
const LIGHT: ThemeTokens = ThemeTokens {
    surface: rgb(0xf2, 0xf3, 0xf5),
    surface_raised: rgb(0xfa, 0xfb, 0xfc),
    surface_sunken: rgb(0xe4, 0xe6, 0xea),
    canvas_backdrop: rgb(0xd8, 0xda, 0xde),
    page: rgb(0xff, 0xff, 0xff),
    text: rgb(0x1a, 0x1c, 0x1f),
    text_muted: rgb(0x55, 0x5b, 0x63),
    border: rgb(0xc2, 0xc6, 0xcc),
    accent: rgb(0x11, 0x5e, 0xc4),
    on_accent: rgb(0xff, 0xff, 0xff),
    focus: rgb(0x6b, 0x3a, 0x00),
    guide: rgb(0x00, 0x6d, 0x9e),
    grid: rgb(0xc2, 0xc6, 0xcc),
    warning: rgb(0x8a, 0x5a, 0x00),
    error: rgb(0xb3, 0x26, 0x26),
};

/// The row height of a list, a table or a property field, in points.
///
/// Twenty points is the professional density bar of `research/05 §2.1`:
/// dense enough to show a real layer tree, tall enough to stay a legible
/// hit target at 1× with a mouse.
pub const ROW_HEIGHT: f32 = 20.0;

/// The height of a ruler strip, in points.
pub const RULER_THICKNESS: f32 = 18.0;

/// The height of the status bar, in points.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;

/// The side of a palette swatch on the colour line, in points.
pub const SWATCH_SIZE: f32 = 16.0;

/// Applies a theme to an egui context: colours, spacing and density.
///
/// This is the only place that touches `egui::Style`. Calling it every frame
/// is cheap and makes a live colour-scheme change — the desktop switching to
/// dark at sunset — take effect on the next frame with no restart.
pub fn apply(ctx: &egui::Context, theme: ResolvedTheme) {
    let t = ThemeTokens::of(theme);
    let mut style = (*ctx.style()).clone();

    style.visuals = match theme {
        ResolvedTheme::Dark => egui::Visuals::dark(),
        ResolvedTheme::Light => egui::Visuals::light(),
    };
    let v = &mut style.visuals;
    v.panel_fill = t.surface;
    v.window_fill = t.surface;
    v.extreme_bg_color = t.surface_sunken;
    v.faint_bg_color = t.surface_raised;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent.linear_multiply(0.45);
    v.selection.stroke = egui::Stroke::new(1.0_f32, t.on_accent);
    v.window_stroke = egui::Stroke::new(1.0_f32, t.border);
    v.widgets.noninteractive.bg_fill = t.surface;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, t.border);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, t.text_muted);
    v.widgets.inactive.bg_fill = t.surface_raised;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, t.text);
    v.widgets.hovered.bg_fill = t.surface_raised;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, t.accent);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, t.text);
    v.widgets.active.bg_fill = t.accent;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, t.on_accent);
    // egui 0.33 paints a focused widget with the `active` visuals, so the
    // focus ring lives here: two points of the focus token, which is not
    // the accent, so "selected" and "focused" never look the same.
    v.widgets.active.bg_stroke = egui::Stroke::new(2.0_f32, t.focus);
    v.widgets.open.bg_fill = t.surface_sunken;

    // Density. Everything here is a departure from egui's defaults and is
    // what the spike measures.
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(4.0, 2.0);
    s.button_padding = egui::vec2(4.0, 1.0);
    s.menu_margin = egui::Margin::same(2);
    s.indent = 14.0;
    s.interact_size = egui::vec2(28.0, ROW_HEIGHT);
    s.slider_width = 96.0;
    s.combo_width = 96.0;
    s.text_edit_width = 120.0;
    s.icon_width = 12.0;
    s.icon_width_inner = 7.0;
    s.scroll.bar_width = 8.0;
    s.scroll.floating = false;

    for font in style.text_styles.values_mut() {
        font.size = (font.size * 0.92).round();
    }

    ctx.set_style(style);
}

/// The relative luminance of a colour, as defined by WCAG 2.1.
///
/// Used by the theme tests: a token set is not a matter of taste when the
/// text sits on the surface at a contrast ratio below 4.5.
pub fn relative_luminance(c: egui::Color32) -> f64 {
    fn channel(v: u8) -> f64 {
        let v = v as f64 / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// The WCAG contrast ratio between two colours, from 1.0 to 21.0.
pub fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMES: [ResolvedTheme; 2] = [ResolvedTheme::Dark, ResolvedTheme::Light];

    #[test]
    fn follow_system_tracks_the_portal_and_an_override_does_not() {
        assert_eq!(
            Theme::FollowSystem.resolve(ColorScheme::Light),
            ResolvedTheme::Light
        );
        assert_eq!(
            Theme::FollowSystem.resolve(ColorScheme::Dark),
            ResolvedTheme::Dark
        );
        assert_eq!(
            Theme::FollowSystem.resolve(ColorScheme::NoPreference),
            ResolvedTheme::Dark
        );
        assert_eq!(
            Theme::Light.resolve(ColorScheme::Dark),
            ResolvedTheme::Light
        );
        assert_eq!(Theme::Dark.resolve(ColorScheme::Light), ResolvedTheme::Dark);
    }

    #[test]
    fn body_text_is_legible_on_every_surface_in_both_schemes() {
        for scheme in SCHEMES {
            let t = ThemeTokens::of(scheme);
            for (name, surface) in [
                ("surface", t.surface),
                ("raised", t.surface_raised),
                ("sunken", t.surface_sunken),
            ] {
                let ratio = contrast_ratio(t.text, surface);
                assert!(ratio >= 4.5, "{scheme:?} text on {name}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn muted_text_clears_the_large_text_bar() {
        for scheme in SCHEMES {
            let t = ThemeTokens::of(scheme);
            let ratio = contrast_ratio(t.text_muted, t.surface);
            assert!(ratio >= 3.0, "{scheme:?} muted text: {ratio:.2}");
        }
    }

    #[test]
    fn the_focus_ring_is_visible_and_is_not_the_accent() {
        for scheme in SCHEMES {
            let t = ThemeTokens::of(scheme);
            assert_ne!(t.focus, t.accent, "{scheme:?}");
            assert!(
                contrast_ratio(t.focus, t.surface) >= 3.0,
                "{scheme:?} focus ring on surface"
            );
            assert!(
                contrast_ratio(t.focus, t.accent) >= 1.5,
                "{scheme:?} focus ring on a selected row"
            );
        }
    }

    #[test]
    fn accent_text_is_legible_on_the_accent() {
        for scheme in SCHEMES {
            let t = ThemeTokens::of(scheme);
            assert!(contrast_ratio(t.on_accent, t.accent) >= 4.5, "{scheme:?}");
        }
    }

    #[test]
    fn guides_and_the_grid_are_distinguishable_from_the_backdrop() {
        for scheme in SCHEMES {
            let t = ThemeTokens::of(scheme);
            assert!(contrast_ratio(t.guide, t.page) >= 1.8, "{scheme:?} guide");
            assert!(contrast_ratio(t.grid, t.page) >= 1.05, "{scheme:?} grid");
        }
    }

    #[test]
    fn applying_a_theme_sets_the_dense_row_height() {
        let ctx = egui::Context::default();
        apply(&ctx, ResolvedTheme::Dark);
        let style = ctx.style();
        assert_eq!(style.spacing.interact_size.y, ROW_HEIGHT);
        assert!(style.spacing.item_spacing.y <= 2.0);
        assert_eq!(style.visuals.panel_fill, DARK.surface);
        // A focused widget must be visibly focused: egui 0.33 styles it
        // with `widgets.active`.
        assert_eq!(style.visuals.widgets.active.bg_stroke.color, DARK.focus);
        assert!(style.visuals.widgets.active.bg_stroke.width >= 2.0);
    }

    #[test]
    fn a_live_scheme_change_takes_effect_on_the_next_frame() {
        let ctx = egui::Context::default();
        apply(&ctx, ResolvedTheme::Dark);
        assert_eq!(ctx.style().visuals.panel_fill, DARK.surface);
        apply(&ctx, ResolvedTheme::Light);
        assert_eq!(ctx.style().visuals.panel_fill, LIGHT.surface);
    }

    #[test]
    fn known_luminance_anchors() {
        assert!((relative_luminance(egui::Color32::WHITE) - 1.0).abs() < 1e-6);
        assert!(relative_luminance(egui::Color32::BLACK) < 1e-6);
        assert!((contrast_ratio(egui::Color32::WHITE, egui::Color32::BLACK) - 21.0).abs() < 1e-6);
    }
}
