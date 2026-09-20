//! Text structure.
//!
//! Story → line → item, as the original and the format both have it, because
//! that is the structure incremental editing and wrapping need. **Structure
//! only**: shaping, measurement and layout are Phase 9, and the metrics they
//! produce are a derived cache, not node data.

use std::sync::Arc;

use xarast_geom::{Matrix, Mp};

/// How a story is laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextLayout {
    /// Free text anchored at a point.
    AtPoint,
    /// Text flowing in a column of a fixed width.
    InColumn {
        /// The column width.
        width: Mp,
        /// Whether words wrap.
        word_wrap: bool,
    },
    /// Text on a path. The path is a child node, so it stays editable.
    OnPath {
        /// Whether the flow direction is reversed.
        reversed: bool,
        /// Whether glyphs rotate with the path.
        tangential: bool,
        /// Indent from the start of the path.
        left_indent: Mp,
        /// Indent from the end of the path.
        right_indent: Mp,
    },
}

/// Paragraph alignment.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum Justification {
    /// Ragged right.
    #[default]
    Left,
    /// Centred.
    Centre,
    /// Ragged left.
    Right,
    /// Both margins flush.
    Full,
}

/// Line spacing, either a ratio of the font size or an absolute distance.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum LineSpacing {
    /// A multiple of the font size.
    Ratio(f32),
    /// An absolute distance.
    Absolute(Mp),
}

impl Default for LineSpacing {
    fn default() -> LineSpacing {
        LineSpacing::Ratio(1.0)
    }
}

/// Superscript and subscript.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Script {
    /// Whether the script offset is applied at all.
    pub on: bool,
    /// Baseline offset as a fraction of the font size.
    pub offset: f32,
    /// Size as a fraction of the font size.
    pub size: f32,
}

/// A text story: the root of a block of text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStoryNode {
    /// The story's placement.
    pub transform: Matrix,
    /// How the text is laid out.
    pub layout: TextLayout,
    /// Whether pair kerning is applied.
    pub auto_kern: bool,
    /// Whether the story prints as outlines rather than as text.
    pub print_as_shapes: bool,
}

impl Default for TextStoryNode {
    fn default() -> TextStoryNode {
        TextStoryNode {
            transform: Matrix::IDENTITY,
            layout: TextLayout::AtPoint,
            auto_kern: true,
            print_as_shapes: false,
        }
    }
}

/// One line of a story.
///
/// Carries no metrics: those are Phase 9's derived cache, kept out of the
/// arena so that a line is comparable and cheap.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TextLineNode {
    /// A tab ruler that applies to this line, when the file carried one.
    pub ruler: Option<Arc<[TabStop]>>,
}

/// One tab stop.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct TabStop {
    /// Where the stop sits.
    pub position: Mp,
    /// The stop's kind, as the format's `type_and_flags` byte gives it.
    pub kind: u8,
}

/// One item inside a line.
///
/// Held inline in [`NodeKind`](crate::NodeKind) — eight bytes — because a text
/// document is mostly these and they must not pay for a spread.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum TextItem {
    /// A character.
    Char(char),
    /// A manual kern.
    Kern(Mp),
    /// A horizontal tab.
    Tab,
    /// An end of line; `true` when it ends a paragraph.
    LineBreak(bool),
}
