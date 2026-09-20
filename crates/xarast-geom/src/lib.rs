//! Geometry primitives shared by every other Xarast crate.
//!
//! The document model stores coordinates as integer millipoints, which is what
//! gives Xarast exact equality, deterministic output and lossless `.xar`
//! round-tripping. Continuous operations — flattening, offsetting, boolean
//! operations — work in `f64` and quantise on the way back.
//!
//! See `docs/phases/phase-01-geometry-and-colour.md`.

#![doc(html_no_source)]
