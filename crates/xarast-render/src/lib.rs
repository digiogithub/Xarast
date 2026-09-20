//! The Xarast render engine.
//!
//! One scene and display-list frontend feeds two interchangeable backends: a
//! GPU backend on `wgpu` and a CPU backend. The CPU backend is not a fallback
//! afterthought — it is the deterministic path used for export and for golden
//! image tests, and comparing the two backends pixel by pixel is itself a test.
//!
//! The compositor is ours because no third-party library implements Xara's
//! blend modes, its conical and diamond gradients, or its non-linear ramp
//! profiles.
//!
//! See `docs/phases/phase-04-render-engine.md`.
