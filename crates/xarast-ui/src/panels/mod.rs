//! The panels the MVP needs, plus the status bar.
//!
//! The layer panel, which is how a Xara document is navigated at all; the
//! colour panel (the colour editor) and the colour gallery (phase 8). The
//! other galleries, the bitmap panel and the live effect panels belong to
//! later phases and are deliberately absent rather than stubbed.

pub mod colour;
pub mod gallery;
pub mod layers;
pub mod status;

pub use colour::ColourPanel;
pub use gallery::ColourGallery;
pub use layers::LayerPanel;
pub use status::{StatusBar, status_bar_ui};
