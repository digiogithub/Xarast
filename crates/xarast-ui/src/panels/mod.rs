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
