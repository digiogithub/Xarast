//! The panels the MVP needs, plus the status bar.
//!
//! The layer panel, which is how a Xara document is navigated at all; the
//! colour panel (the colour editor) and the colour gallery (phase 8); the
//! bitmap gallery and the photo panel (phase 10). The other galleries and
//! the live effect panels belong to later phases and are deliberately
//! absent rather than stubbed.

pub mod bitmaps;
pub mod colour;
pub mod gallery;
pub mod layers;
pub mod photo;
pub mod status;

pub use bitmaps::BitmapGallery;
pub use colour::ColourPanel;
pub use gallery::ColourGallery;
pub use layers::LayerPanel;
pub use photo::PhotoPanel;
pub use status::{StatusBar, status_bar_ui};

/// The value a slider showing `decimals` decimals is handed.
///
/// An `egui::Slider` with the default clamping rounds its value to the
/// decimals it shows **every frame**, and reports that as a change: a
/// stored `0.4_f32` tint, 40.000000596 %, would otherwise come back as a
/// `Set` of the same colour each frame the panel is drawn. Rounding it
/// here first leaves the slider nothing to change until someone moves it.
pub(crate) fn slider_value(value: f64, decimals: usize) -> f64 {
    egui::emath::round_to_decimals(value, decimals)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_rounded_value_is_what_the_slider_would_make_of_it() {
        assert_eq!(super::slider_value(f64::from(0.4_f32) * 100.0, 0), 40.0);
        assert_eq!(super::slider_value(f64::from(1.3_f32), 2), 1.3);
    }
}
