//! What an export did, and what it could not do.
//!
//! **Rule:** every lossy decision an exporter makes is a [`Compromise`]
//! in the report. New lossy behaviour adds a variant; it is never silent.

use std::sync::Arc;
use std::time::Duration;

use xarast_color::Rgba8;
use xarast_render::{BlendFamily, SceneNodeId};

use crate::model::SizingError;
use crate::options::FormatId;

/// Honest accounting of an export.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportReport {
    /// Bytes in the written file.
    pub bytes_written: u64,
    /// Wall time, end to end.
    pub duration: Duration,
    /// Time building the scene.
    pub scene_time: Duration,
    /// Time rasterising.
    pub render_time: Duration,
    /// Time encoding and writing.
    pub encode_time: Duration,
    /// The pixel size written.
    pub pixels: (u32, u32),
    /// The resolution written, in dots per inch.
    pub dpi: f64,
    /// Display-list commands drawn.
    pub commands: usize,
    /// Everything that was not reproduced exactly.
    pub compromises: Vec<Compromise>,
}

/// One thing an export could not reproduce exactly.
#[derive(Debug, Clone, PartialEq)]
pub enum Compromise {
    /// Transparency was requested but the format or colour type has no
    /// alpha: the image was composited onto `onto`.
    AlphaFlattened {
        /// The colour it was composited onto.
        onto: Rgba8,
    },
    /// The scene builder could not draw some content (text awaiting its
    /// fonts, a failed image, an unsupported clip, a live effect).
    NotRendered {
        /// What kind of content.
        what: Arc<str>,
        /// How many objects.
        count: usize,
    },
    /// A font the document asked for was replaced.
    FontSubstituted {
        /// The family asked for.
        requested: Arc<str>,
        /// The family used.
        used: Arc<str>,
    },
    /// A 16-bit PNG was requested: the render is 8-bit, so the extra bits
    /// carry no information (`v × 257`).
    WidenedFrom8Bit,
    /// A vector format could not express an object, so it was rendered
    /// to an image on the CPU backend and placed (the fidelity ladder's
    /// last step).
    Rasterised {
        /// The object: the scene node, which is the document tag.
        node: SceneNodeId,
        /// Why.
        reason: Arc<str>,
        /// At what resolution.
        dpi: f64,
    },
    /// A vector format expressed an object in a different construct that
    /// is close but not identical (a conical gradient as a wedge fan, a
    /// three-colour mesh as a sampled grid).
    Approximated {
        /// The object.
        node: SceneNodeId,
        /// What was approximated, and how.
        what: Arc<str>,
    },
    /// A transparency family was mapped to a same-shaped but differently
    /// defined blend mode of the target format.
    BlendModeApproximated {
        /// The object.
        node: SceneNodeId,
        /// Our family.
        ours: BlendFamily,
        /// The target's mode.
        theirs: Arc<str>,
    },
}

impl std::fmt::Display for Compromise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Compromise::AlphaFlattened { onto } => write!(
                f,
                "transparency flattened onto #{:02x}{:02x}{:02x}",
                onto.r, onto.g, onto.b
            ),
            Compromise::NotRendered { what, count } => write!(f, "{what}: {count} not reproduced"),
            Compromise::FontSubstituted { requested, used } => {
                write!(f, "font {requested} replaced by {used}")
            }
            Compromise::WidenedFrom8Bit => write!(f, "16-bit samples widened from 8-bit"),
            Compromise::Rasterised { node, reason, dpi } => {
                write!(f, "object {} rasterised at {dpi} dpi: {reason}", node.0)
            }
            Compromise::Approximated { node, what } => {
                write!(f, "object {} approximated: {what}", node.0)
            }
            Compromise::BlendModeApproximated { node, ours, theirs } => {
                write!(f, "object {}: {ours:?} drawn as {theirs}", node.0)
            }
        }
    }
}

/// Why an export failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// A format this build will not write. For `.xar` this is permanent:
    /// architecture §3.5.
    #[error("{id}: {reason}")]
    UnsupportedFormat {
        /// What was asked for.
        id: String,
        /// Why not.
        reason: &'static str,
    },
    /// An option that needs a Cargo feature this build lacks.
    #[error("not built with {0}")]
    FeatureNotBuilt(&'static str),
    /// Options that do not fit the format or the image.
    #[error("{format:?}: {reason}")]
    BadOptions {
        /// The format.
        format: FormatId,
        /// What is wrong.
        reason: String,
    },
    /// The area could not be resolved (no selection, no such page).
    #[error("the export area is not available: {0}")]
    Area(String),
    /// The sizing is out of range.
    #[error(transparent)]
    Sizing(#[from] SizingError),
    /// The scene could not be built.
    #[error("the scene could not be built: {0}")]
    Scene(String),
    /// The rasteriser refused.
    #[error("render: {0}")]
    Render(String),
    /// The encoder failed.
    #[error("encode: {0}")]
    Encode(String),
    /// The file could not be written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The caller cancelled; nothing was written.
    #[error("cancelled")]
    Cancelled,
}
