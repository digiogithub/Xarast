//! The photo panel's model (phase 10, T10.6.8, and the live preview of
//! T10.6.5).
//!
//! The panel edits the photo chain ([`xarast_doc::PhotoOps`]) of the one
//! selected bitmap object. What it shows is a [`PhotoPanelView`]; what it
//! asks for is a [`PhotoPanelOp`], carried by
//! [`crate::Intent::PhotoPanel`]. Every edit reaches the document through
//! [`Session::set_photo_ops`], so each one is a single "Adjust Photo" undo
//! step.
//!
//! # A slider drag previews; the release commits once
//!
//! As the fill tool does (`docs/memory/tools.md`, decision 45), a drag
//! never touches the document: each [`PhotoPanelOp::Preview`] puts the
//! chain in the session's [`crate::tool::Preview::photo`], which the
//! walker evaluates on a reduced level of the master (the *proxy*, never
//! more than about two megapixels) instead of at full resolution.
//! [`PhotoPanelOp::Commit`] applies the last previewed chain as one step;
//! [`PhotoPanelOp::Cancel`] (and `Esc`, undo, redo) drops it, and nothing
//! ever happened.
//!
//! A preview may change only the point operations and the channel mixes.
//! A chain whose crop or orientation differs from the object's would draw
//! into the old placement, so such a change is committed at once
//! ([`PhotoPanelOp::Set`]) — the panel edits crop and orientation with
//! typed fields and buttons, never with a drag.

use xarast_doc::{NodeId, NodeKind};

/// The chain's vocabulary, for the interface crate (which does not depend
/// on `xarast-doc`).
pub use xarast_doc::photo::{
    GAMMA_RANGE, Levels, LevelsChannel, PhotoOp, PhotoOps, PhotoOrient, PixelRect,
};

use crate::intent::Changed;
use crate::session::{Session, SessionError};

/// What the photo panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PhotoPanelView {
    /// The bitmap object being adjusted; `None` when the selection is not
    /// exactly one bitmap object.
    pub node: Option<NodeId>,
    /// How many objects are selected (to say why nothing is editable).
    pub selected: usize,
    /// The bitmap's name.
    pub name: String,
    /// The master's size in pixels, when known.
    pub master: Option<(u32, u32)>,
    /// The chain shown: the one being previewed during a drag, else the
    /// object's own.
    pub ops: PhotoOps,
    /// False when the chain holds an operation this version does not
    /// know: the panel shows it and edits nothing.
    pub editable: bool,
    /// Whether a slider drag is in flight.
    pub dragging: bool,
}

impl PhotoPanelView {
    /// The kinds of the operations this version cannot edit, in order.
    #[must_use]
    pub fn unknown_kinds(&self) -> Vec<String> {
        self.ops
            .ops
            .iter()
            .filter(|o| o.rank().is_none())
            .map(|o| o.kind_name().to_owned())
            .collect()
    }
}

/// What the photo panel asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum PhotoPanelOp {
    /// A live value of a drag: shown at proxy resolution, not applied.
    /// A chain whose crop or orientation differs is applied at once, as
    /// [`PhotoPanelOp::Set`].
    Preview(PhotoOps),
    /// Ends the drag in flight: its last chain becomes one undo step.
    Commit,
    /// Abandons the drag in flight: nothing changes.
    Cancel,
    /// A change made in one go (a typed number, a key, a button): one
    /// undo step.
    Set(PhotoOps),
    /// Removes every operation: one undo step.
    Reset,
}

/// The one bitmap object the panel edits: the selection when it is
/// exactly one bitmap object.
#[must_use]
pub fn target(session: &Session) -> Option<NodeId> {
    let mut sel = session.edit.selection();
    let node = sel.next()?;
    if sel.next().is_some() {
        return None;
    }
    matches!(session.doc.tree.kind(node), Some(NodeKind::Bitmap(_))).then_some(node)
}

/// The panel's view of `session`.
#[must_use]
pub fn view(session: &Session) -> PhotoPanelView {
    let selected = session.edit.selection_len();
    let empty = PhotoPanelView {
        node: None,
        selected,
        name: String::new(),
        master: None,
        ops: PhotoOps::new(),
        editable: false,
        dragging: false,
    };
    let Some(node) = target(session) else {
        return empty;
    };
    let Some(NodeKind::Bitmap(bm)) = session.doc.tree.kind(node) else {
        return empty;
    };
    let live = session
        .preview
        .photo
        .iter()
        .find(|(n, _)| *n == node)
        .map(|(_, o)| o.clone());
    let name = session
        .doc
        .resources
        .bitmap(bm.image)
        .map(|r| r.name.to_string())
        .unwrap_or_default();
    PhotoPanelView {
        node: Some(node),
        selected,
        name,
        master: crate::place::bitmap_pixels(&session.doc, bm.image).map(|(size, _)| size),
        dragging: live.is_some(),
        ops: live.unwrap_or_else(|| bm.photo_ops.clone()),
        editable: bm.photo_ops.is_editable(),
    }
}

/// Carries out one panel operation.
///
/// # Errors
///
/// What [`Session::set_photo_ops`] returns: a locked object, a chain this
/// version cannot edit.
pub(crate) fn run(session: &mut Session, op: PhotoPanelOp) -> Result<Changed, SessionError> {
    match op {
        PhotoPanelOp::Preview(ops) => preview(session, ops),
        PhotoPanelOp::Commit => {
            let Some((node, ops)) = take_drag(session) else {
                return Ok(Changed::empty());
            };
            session.set_photo_ops(node, &ops)?;
            Ok(Changed::DOCUMENT | Changed::UI | Changed::SELECTION)
        }
        PhotoPanelOp::Cancel => Ok(settle(session)),
        PhotoPanelOp::Set(ops) => set(session, &ops),
        PhotoPanelOp::Reset => set(session, &PhotoOps::new()),
    }
}

/// Drops a drag in flight, showing the document as it is. What undo,
/// redo, `Esc` and a new document do first.
pub(crate) fn settle(session: &mut Session) -> Changed {
    if take_drag(session).is_some() {
        Changed::DOCUMENT | Changed::UI
    } else {
        Changed::empty()
    }
}

fn take_drag(session: &mut Session) -> Option<(NodeId, PhotoOps)> {
    let photo = std::mem::take(&mut session.preview.photo);
    photo.into_iter().next()
}

fn set(session: &mut Session, ops: &PhotoOps) -> Result<Changed, SessionError> {
    let mut changed = settle(session);
    if let Some(node) = target(session)
        && session.set_photo_ops(node, ops)?.is_some()
    {
        changed |= Changed::DOCUMENT | Changed::UI | Changed::SELECTION;
    }
    Ok(changed)
}

fn preview(session: &mut Session, ops: PhotoOps) -> Result<Changed, SessionError> {
    let Some(node) = target(session) else {
        return Ok(settle(session));
    };
    let Some(NodeKind::Bitmap(bm)) = session.doc.tree.kind(node) else {
        return Ok(Changed::empty());
    };
    if !bm.photo_ops.is_editable() || crate::ops::on_locked_layer(&session.doc, node) {
        return Ok(Changed::empty());
    }
    let ops = ops.normalised();
    if ops.crop() != bm.photo_ops.crop() || ops.orient() != bm.photo_ops.orient() {
        return set(session, &ops);
    }
    let entry = (node, ops);
    if session.preview.photo.first() == Some(&entry) && session.preview.photo.len() == 1 {
        return Ok(Changed::empty());
    }
    session.preview.photo = vec![entry];
    Ok(Changed::DOCUMENT | Changed::UI)
}

/// A short description of one operation, as the panel's chain list reads
/// it: "Brightness +20 %", "Levels (red) 10–240 → 0–255".
#[must_use]
pub fn op_label(op: &PhotoOp) -> String {
    let pct = |v: f32| format!("{:+.0} %", f64::from(v) * 100.0);
    match op {
        PhotoOp::Crop(r) => format!("Crop {} × {} at ({}, {})", r.width, r.height, r.x, r.y),
        PhotoOp::Orient(o) => {
            let turn = match o.turns % 4 {
                0 => None,
                1 => Some("turned 90° clockwise"),
                2 => Some("turned 180°"),
                _ => Some("turned 90° anticlockwise"),
            };
            match (o.flip, turn) {
                (true, Some(t)) => format!("Mirrored, {t}"),
                (true, None) => "Mirrored".to_owned(),
                (false, Some(t)) => format!("Orientation: {t}"),
                (false, None) => "Orientation unchanged".to_owned(),
            }
        }
        PhotoOp::Levels(l) => format!(
            "Levels ({}) {}–{} → {}–{}",
            channel_label(l.channel),
            l.in_lo,
            l.in_hi,
            l.out_lo,
            l.out_hi
        ),
        PhotoOp::Gamma(g) => format!("Gamma {g:.2}"),
        PhotoOp::Brightness(v) => format!("Brightness {}", pct(*v)),
        PhotoOp::Contrast(v) => format!("Contrast {}", pct(*v)),
        PhotoOp::Saturation(v) => format!("Saturation {}", pct(*v)),
        PhotoOp::Greyscale => "Greyscale".to_owned(),
        PhotoOp::Unknown { kind, .. } => format!("Unknown operation \u{2018}{kind}\u{2019}"),
    }
}

/// What the panel calls a levels channel.
#[must_use]
pub const fn channel_label(c: LevelsChannel) -> &'static str {
    match c {
        LevelsChannel::All => "all channels",
        LevelsChannel::Red => "red",
        LevelsChannel::Green => "green",
        LevelsChannel::Blue => "blue",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_has_a_readable_label() {
        let ops = [
            PhotoOp::Crop(PixelRect {
                x: 1,
                y: 2,
                width: 30,
                height: 40,
            }),
            PhotoOp::Orient(PhotoOrient::CW),
            PhotoOp::Orient(PhotoOrient::FLIP_V),
            PhotoOp::Levels(Levels {
                channel: LevelsChannel::Red,
                in_lo: 10,
                in_hi: 240,
                out_lo: 0,
                out_hi: 255,
            }),
            PhotoOp::Gamma(1.4),
            PhotoOp::Brightness(0.2),
            PhotoOp::Contrast(-0.25),
            PhotoOp::Saturation(-1.0),
            PhotoOp::Greyscale,
            PhotoOp::Unknown {
                kind: "curves".into(),
                raw: "<x/>".into(),
            },
        ];
        let labels: Vec<String> = ops.iter().map(op_label).collect();
        assert_eq!(
            labels,
            [
                "Crop 30 × 40 at (1, 2)",
                "Orientation: turned 90° clockwise",
                "Mirrored, turned 180°",
                "Levels (red) 10–240 → 0–255",
                "Gamma 1.40",
                "Brightness +20 %",
                "Contrast -25 %",
                "Saturation -100 %",
                "Greyscale",
                "Unknown operation \u{2018}curves\u{2019}",
            ]
        );
    }
}
