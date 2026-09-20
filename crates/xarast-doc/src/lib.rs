//! The Xarast document model.
//!
//! A generational arena of nodes forms the document tree; paint order is tree
//! order. Attributes are nodes with lexical scope over their following
//! siblings, which is what makes grouping preserve appearance and what both
//! `.xar` and SVG already assume.
//!
//! Every mutation goes through the command bus, which records its own inverse.
//! Nothing else may mutate the arena — that rule is what makes undo complete by
//! construction.
//!
//! This crate has no graphics dependencies so that it can be tested, fuzzed and
//! benchmarked with no GPU and no windowing system present.
//!
//! See `docs/phases/phase-02-document-model.md` and
//! `docs/memory/document-model.md`.
