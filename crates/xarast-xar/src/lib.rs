//! Read-only importer for the legacy Xara `.xar` format.
//!
//! Writing `.xar` is an explicit non-goal: we understand the format by
//! observation, and emitting it would invite silently corrupt files.
//!
//! This crate parses hostile input by definition. It must never panic, never
//! overflow and never allocate unboundedly, whatever the bytes say. It is
//! fuzzed from day one.
//!
//! See `docs/phases/phase-03-xar-importer.md` and
//! `docs/research/01-xar-format.md`.
