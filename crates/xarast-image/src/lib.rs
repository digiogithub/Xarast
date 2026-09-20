//! Image decoding, encoding, resampling and bitmap resources.
//!
//! Decoding untrusted images is attack surface: every entry point here bounds
//! its allocation before decoding.
//!
//! See `docs/phases/phase-10-bitmaps-and-photo.md`.
