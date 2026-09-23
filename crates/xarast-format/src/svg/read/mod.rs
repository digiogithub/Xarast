//! The Xarast SVG profile, reading side (`research/06 §5`, `§6`, `§8`;
//! phase 6 workstream W4).
//!
//! [`read_svg`] turns the text of `document.svg` back into a
//! [`Document`]. It reads what [`write_svg`](crate::svg::write_svg)
//! writes exactly — the round trip is tested over the whole corpus on a
//! normal form ([`normal_form`]) and on the bytes of a second save — and
//! it reads what other programs make of such a file tolerantly: SVG
//! inheritance, `style`, class rules, transforms, every path command, plain
//! `rect`/`circle`/`line` elements, gradients with `href` chains.
//!
//! # What goes where
//!
//! | Module | What |
//! |---|---|
//! | [`dom`] | F4.1: the preserving XML layer — spans, namespaces, limits, no DTD |
//! | [`parse`] | numbers (exact millipoints), path data, transforms, colours |
//! | [`style`] | the SVG property cascade |
//! | `build` | F4.2–F4.4: elements → nodes, foreign baggage, the twins |
//! | `build::paint` | fills, strokes, transparencies: the inverse of the writer's `paint` |
//! | [`normal`] | XARA-T-0105: the normal form model equivalence is judged on |
//!
//! # Rules the reader keeps
//!
//! - **The parametric twin wins** over the base SVG (`§5.1` rule 2), except
//!   under `xarast:base-authoritative="true"` (rule 4).
//! - **Baked subtrees** (`xarast:generated`) whose generator is present
//!   are kept as generated nodes (nothing regenerates live effects before
//!   Phase 13, which is why the writer marks them base-authoritative);
//!   an orphan one becomes an ordinary group of editable geometry, with a
//!   warning (F4.3). Art is never deleted without a replacement.
//! - **Everything not understood is kept on the node it was read on**
//!   (F4.4): unknown attributes, unknown child elements, comments and
//!   processing instructions, verbatim, with their position among the known
//!   children. An element the reader cannot interpret (say a `<path>` whose
//!   data is corrupt) is kept as foreign data too, rather than dropped.
//! - **Active content is stripped** (`§5.3`): `<script>`, `<foreignObject>`,
//!   SMIL and `on*` attributes, and any foreign fragment containing them,
//!   with a warning.

pub mod dom;
pub mod normal;
pub mod parse;
pub mod style;

mod build;

use std::sync::Arc;

use xarast_doc::{BuildError, BuildLimits, Diagnostic, Document};

pub(crate) use build::unix_of_rfc3339;
pub use dom::{XmlError, XmlLimits};
pub use normal::normal_form;

use crate::limits::Limits;

/// Options for [`read_svg`].
#[derive(Debug, Clone)]
pub struct ReadOptions {
    /// Container limits; `max_xml_depth` bounds the nesting.
    pub limits: Limits,
    /// Model limits: nodes, bytes, points per path.
    pub build: BuildLimits,
    /// Largest `document.svg` accepted, in bytes.
    pub max_svg_bytes: usize,
    /// Most XML elements accepted.
    pub max_elements: usize,
}

impl Default for ReadOptions {
    fn default() -> ReadOptions {
        ReadOptions {
            limits: Limits::DEFAULT,
            build: BuildLimits::default(),
            max_svg_bytes: 2 << 30,
            max_elements: 40_000_000,
        }
    }
}

impl ReadOptions {
    /// Tight limits for fuzzing.
    #[must_use]
    pub fn fuzz() -> ReadOptions {
        ReadOptions {
            limits: Limits::FUZZ,
            build: BuildLimits::small(),
            max_svg_bytes: 4 << 20,
            max_elements: 100_000,
        }
    }
}

/// Why `document.svg` could not be read at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SvgReadError {
    /// The XML is not acceptable.
    #[error(transparent)]
    Xml(#[from] XmlError),
    /// The root element is not `<svg>` in the SVG namespace.
    #[error("the root element is not an SVG <svg>")]
    NotSvg,
    /// The model refused the document.
    #[error(transparent)]
    Build(#[from] BuildError),
}

/// What a read found, beyond the document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct ReadStats {
    /// Elements that became nodes.
    pub nodes: usize,
    /// Attribute nodes the reader created (one per non-default slot per
    /// object: XARA-T-0105's localisation).
    pub attributes: usize,
    pub foreign_attributes: usize,
    pub foreign_elements: usize,
    /// Unknown elements in the SVG namespace: they affect rendering.
    pub foreign_svg_elements: usize,
    pub comments: usize,
    pub processing_instructions: usize,
    /// Elements, attributes and fragments removed for security.
    pub stripped: usize,
    /// Elements the reader could not interpret and kept as foreign data.
    pub uninterpretable: usize,
    /// Twins that won over the base SVG.
    pub parametric: usize,
    /// Baked subtrees kept because their generator is present (nothing
    /// regenerates them yet).
    pub generated_kept: usize,
    /// Baked subtrees whose generator is gone, kept as plain geometry.
    pub generated_orphaned: usize,
    /// Ids that were duplicated or not ours, reassigned.
    pub ids_reassigned: usize,
    /// Quick shapes whose stored outline (their `d`) differs from the one
    /// `QuickShape::outline` generates from the parameters; the stored one
    /// is kept as the outline cache. The `.xar` importer generates edge
    /// templates in the shape's own space, which `outline` only
    /// approximates, so most of these are exact outlines, not edits
    /// (ProbeX16: 31,936). The parameters stay the source of truth.
    pub quickshape_outlines_kept: usize,
}

/// The preservation digest of `research/06 §8.4`: what the file declared
/// and what the reader found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preservation {
    /// `xarast:foreign-count`, when present.
    pub declared_count: Option<usize>,
    /// `xarast:foreign-digest`, when present and well formed.
    pub declared_digest: Option<[u8; 32]>,
    /// Foreign items now in the model.
    pub count: usize,
    /// The digest of those items, computed the way the writer does.
    pub digest: [u8; 32],
}

impl Preservation {
    /// Items the file says it had and the model no longer has: data another
    /// application destroyed (the §8.4 warning).
    #[must_use]
    pub fn lost(&self) -> usize {
        self.declared_count
            .map_or(0, |d| d.saturating_sub(self.count))
    }

    /// Whether the foreign data is exactly what the writer declared.
    #[must_use]
    pub fn intact(&self) -> bool {
        match (self.declared_count, self.declared_digest) {
            (None, None) => self.count == 0,
            (Some(c), Some(d)) => c == self.count && d == self.digest,
            _ => false,
        }
    }
}

/// The result of [`read_svg`].
#[derive(Debug)]
pub struct SvgRead {
    /// The document.
    pub document: Document,
    /// Warnings: nothing here stopped the read.
    pub diagnostics: Vec<Diagnostic>,
    /// What was read.
    pub stats: ReadStats,
    /// The preservation digest check.
    pub preservation: Preservation,
}

/// Where the reader gets the bytes of a package entry an `href` names
/// (`resources/images/…`). `None` when there is no such entry.
pub type ResourceFetch<'a> = dyn FnMut(&str) -> Option<Arc<[u8]>> + 'a;

/// Reads `document.svg` into a document.
///
/// # Errors
///
/// [`SvgReadError`] when the XML is unacceptable (malformed, a DTD, over a
/// limit), the root is not SVG, or the model refuses what was built.
/// Everything short of that is a [`Diagnostic`].
pub fn read_svg(
    input: &[u8],
    opts: &ReadOptions,
    resources: &mut ResourceFetch<'_>,
) -> Result<SvgRead, SvgReadError> {
    let dom = dom::parse(
        input,
        XmlLimits {
            max_depth: opts.limits.max_xml_depth,
            max_elements: opts.max_elements,
            max_bytes: opts.max_svg_bytes,
        },
    )?;
    build::build(&dom, opts, resources)
}
