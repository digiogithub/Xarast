//! Reader and writer for the native `.xarast` container.
//!
//! A ZIP container wrapping an SVG document plus deduplicated binary
//! resources. A `.xarast` file must open with graceful degradation in a browser
//! or in Inkscape, and with full fidelity here.
//!
//! Data written by a future version and not understood by this one is preserved
//! verbatim across a load/save cycle.
//!
//! See `docs/phases/phase-06-xarast-format.md`.
