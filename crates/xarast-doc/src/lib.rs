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
//!
//! # The shape of the crate
//!
//! | Module | What it owns |
//! |---|---|
//! | [`tree`] | [`Tree`], [`NodeId`], [`NodeData`], the sibling list, traversal, `validate()` |
//! | [`kind`] | [`NodeKind`], the sum type that replaces the original's class hierarchy |
//! | [`attr`] | [`AttrSlot`], [`AttrValue`], [`AttrStack`], [`AttrResolver`] |
//! | [`fill`] | [`FillGeometry`], one generic type in place of ~60 parallel classes |
//! | [`resources`] | [`DocumentResources`]: bitmaps, dashes, arrows, the colour table |
//! | [`history`] | [`Action`], [`Tx`], [`History`], [`Command`], [`CommandBus`] |
//! | [`builder`] | [`DocumentBuilder`], the only public construction path |
//!
//! # The two rules that hold the design together
//!
//! **Mutation is crate private.** [`Tree::attach`](Tree) and its siblings are
//! `pub(crate)`. The only public ways to change a document are
//! [`DocumentBuilder`] while it is being constructed and [`CommandBus`]
//! afterwards. A tool that cannot reach `&mut` the arena cannot make an edit
//! the undo log does not know about.
//!
//! ```compile_fail,E0624
//! let mut doc = xarast_doc::Document::new_empty();
//! let root = doc.tree.root();
//! // `detach` is pub(crate): a tool cannot get at the arena.
//! doc.tree.detach(root).unwrap();
//! ```
//!
//! **Coordinates are millipoints.** Everything positional in the model is
//! [`xarast_geom::Mp`]; floats appear only where colour and gradient stops
//! already are.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc(html_no_source)]

pub mod attr;
pub mod bounds;
pub mod builder;
pub mod digest;
pub mod document;
pub mod fill;
pub mod foreign;
pub mod history;
pub mod kind;
pub mod live;
pub mod resources;
pub mod snapshot;
pub mod structure;
pub mod synth;
pub mod text;
pub mod text_model;
pub mod tree;
pub mod validate;
pub mod walk;

#[cfg(test)]
mod tests;

pub use attr::{
    ALL_ATTR_SLOTS, ATTR_SLOT_COUNT, AttrNode, AttrResolver, AttrSlot, AttrStack, AttrValue,
    DefaultAttrs, MultiAttr, Quality, ResolvedAttrs, default_for, slot_name,
};
pub use bounds::{BoundsCache, Epoch, compute_bounds};
pub use builder::{
    BuildError, BuildId, BuildLimits, DiagCode, Diagnostic, DocumentBuilder, Severity, legal_child,
};
pub use digest::{Canon, CanonicalHasher};
pub use document::{Document, DocumentMeta, DumpOptions};
pub use fill::{
    FillGeometry, Paint, Perspective, ProceduralParams, Ramp, RampMapping, RampStop, Tiling,
    TranspPaint,
};
pub use foreign::{ForeignAttr, ForeignBaggage, ForeignChild, ForeignChildKind, ForeignMarks};
pub use history::{Action, CoalesceKey, Command, CommandBus, EditError, History, Transaction, Tx};
pub use kind::{
    ArrowSpec, BitmapNode, BrushRef, ClipViewMode, ClipViewNode, GroupNode, GuidelineNode,
    NodeKind, OpaqueNode, PathNode, QuickShape, ShapeKind, ShapeNode, StrokeDef, TypefaceRef,
    WidthProfile,
};
pub use live::{
    BevelParams, BevelType, BlendParams, BrushParams, ContourParams, EffectParams, LiveKind,
    LiveNode, LiveRole, MouldKind, MouldParams, RegenState, ShadowKind, ShadowParams,
};
pub use resources::{
    ArrowId, BitmapData, BitmapId, BitmapInfo, BitmapResource, DashId, DocumentResources,
    ImageFormat, OriginalEncoded, ProceduralSource, ResourceRef, collect_unused,
};
pub use snapshot::Snapshot;
pub use structure::{
    AnimProps, DocumentNode, FrameProps, GridKind, GridNode, LayerNode, PageNode, SpreadNode,
};
pub use synth::{SynthSpec, synthetic_document};
pub use text::{
    Justification, LineSpacing, Script, TabStop, TextItem, TextLayout, TextLineNode, TextStoryNode,
};
pub use text_model::{
    CharRun, ItemEntry, KernAt, LineEntry, StoryFlow, StoryText, TextCursor, TextPos,
};
pub use tree::{Attach, Links, NodeData, NodeFlags, NodeId, Tag, Tree, TreeError};
pub use validate::{Invariant, ValidationReport};
pub use walk::{Ancestors, Children, Descend, Postorder, Preorder, RenderWalk, WalkEvent};
