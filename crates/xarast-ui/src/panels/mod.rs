//! The panels the MVP needs, plus the status bar.
//!
//! Two panels make the viewer usable: the layer panel, which is how a Xara
//! document is navigated at all, and the colour panel with its on-screen
//! colour line. Everything else — galleries, the bitmap panel, the live
//! effect panels — belongs to later phases and is deliberately absent
//! rather than stubbed.

pub mod colour;
pub mod layers;
pub mod status;

pub use colour::ColourPanel;
pub use layers::LayerPanel;
pub use status::{StatusBar, status_bar_ui};
