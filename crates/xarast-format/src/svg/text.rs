//! Text stories in the profile (`research/06 §6.7`): what the writer, the
//! reader and the normal form share.
//!
//! A story is written as `<text xarast:exact="true">` holding one `<tspan>`
//! per `TextLine`, and inside each line one `<tspan>` per **run**: a
//! maximal sequence of the line's items whose resolved attributes write
//! the same (text attributes, paint and its twins). The run's start tag
//! carries everything the model needs — the text attributes as SVG
//! properties plus `xarast:` twins for what SVG has no property for — and
//! its content is the items in order: characters as text, and
//! `<xarast:kern>`, `<xarast:eol>` and `<xarast:char>` for the items that
//! are not characters a browser should draw. A reader rebuilds the line's
//! items in order and, before each run, the attributes that differ from
//! the previous run's (the model then resolves every item exactly as it
//! did before the save).
//!
//! The *base* SVG places the characters where the application's layout
//! does when the writer is given a [`TextPlacer`] (per-character `x` / `y`
//! lists on each run); without one, each line starts at the story's origin,
//! one line height below the previous one, which is what the profile wrote
//! before Phase 9.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use xarast_doc::{AttrSlot, AttrStack, AttrValue, Document, NodeId, TabStop};
use xarast_geom::Mp;

use super::num::{f32s, mp};

/// Where the application's layout puts a story's characters, for the base
/// SVG of text (`research/06 §6.7`). The writer knows no fonts: the
/// application, which lays text out to draw it, provides this.
pub trait TextPlacer: Send + Sync {
    /// Places the characters of the story `story`. `attrs` is the attribute
    /// state in force at the story, as a render walk has it on visiting the
    /// story; an implementation must leave it as it found it. `None` when
    /// the story cannot be laid out (the writer then falls back to one line
    /// per `TextLine` at the story's origin).
    fn place(&self, doc: &Document, story: NodeId, attrs: &mut AttrStack)
    -> Option<StoryPlacement>;
}

/// A laid-out story, reduced to what the SVG base needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoryPlacement {
    /// The position of each character item (`TextItem::Char` or `Tab`) in
    /// story space (y up, the first baseline at 0): the left edge of its
    /// box on its baseline, shifts included; for a story placed along its
    /// path, the origin of its glyphs on the path. Items not listed are
    /// placed after the previous one.
    pub chars: HashMap<NodeId, (Mp, Mp)>,
    /// Families that were not available, with the family used instead.
    pub substitutions: Vec<(Arc<str>, Arc<str>)>,
    /// Whether a story on a path was placed along its path (T9.5.6): each
    /// character at its own position, turned by its entry in `rotations`.
    /// `false` for a story on a path laid out on straight lines.
    pub along_path: bool,
    /// For a story placed along its path, the turn of each character item
    /// about its position, in degrees counter-clockwise (y up). Items not
    /// listed do not turn.
    pub rotations: HashMap<NodeId, f64>,
}

/// A shared [`TextPlacer`] in [`super::SvgOptions`], compared by identity.
#[derive(Clone)]
pub struct Placer(pub Arc<dyn TextPlacer>);

impl fmt::Debug for Placer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Placer(..)")
    }
}

impl PartialEq for Placer {
    fn eq(&self, other: &Placer) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Placer {}

/// A ruler as the profile writes it: `position:kind` pairs, the position
/// in points and the kind the format's `type_and_flags` byte.
pub(crate) fn ruler_text(stops: &[TabStop]) -> String {
    let v: Vec<String> = stops
        .iter()
        .map(|t| format!("{}:{}", mp(i64::from(t.position.raw())), t.kind))
        .collect();
    v.join(" ")
}

/// The generic family a browser falls back to, from the PANOSE bytes the
/// way the application's substitution ladder reads them: family kind 2
/// (Latin text) with proportion 9 is monospaced; serif styles 11–13 are
/// sans serif, the others serif. Without PANOSE, `sans-serif`.
pub(crate) fn generic_family(panose: Option<[u8; 10]>) -> &'static str {
    match panose {
        Some(p) if p[0] == 2 && p[3] == 9 => "monospace",
        Some(p) if p[0] == 2 && (11..=13).contains(&p[1]) => "sans-serif",
        Some(p) if p[0] == 2 && p[1] >= 2 => "serif",
        _ => "sans-serif",
    }
}

/// The attributes a run's start tag carries for its text attributes, in
/// the order written: the SVG properties a browser uses, then the twins
/// (`research/06 §6.7`). `substitute` is the family the application used
/// instead of the requested one, when it had to.
///
/// Every value here is resolved from `a`; a reader rebuilds the model's
/// values from these alone (`read/build/ink.rs`).
pub(crate) fn run_text_attrs(
    a: &AttrStack,
    substitutes: &[(Arc<str>, Arc<str>)],
) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::with_capacity(8);
    let (family, full, panose) = match a.get(AttrSlot::TxtFontTypeface) {
        AttrValue::FontTypeface(t) => (Arc::clone(&t.family), Arc::clone(&t.full_name), t.panose),
        _ => (Arc::from(""), Arc::from(""), None),
    };
    let size = match a.get(AttrSlot::TxtFontSize) {
        AttrValue::FontSize(v) => i64::from(v.raw()),
        _ => 0,
    };
    let script = match a.get(AttrSlot::TxtScript) {
        AttrValue::Script(s) => *s,
        _ => xarast_doc::Script::default(),
    };
    // What a browser should draw at: the script's size applies.
    let drawn = if script.on && script.size.is_finite() && script.size > 0.0 {
        (size as f64 * f64::from(script.size)).round() as i64
    } else {
        size
    };
    let substitute = substitutes
        .iter()
        .find(|(from, _)| **from == *family)
        .map(|(_, to)| Arc::clone(to));
    let mut chain = String::new();
    for f in [Some(&family), substitute.as_ref()].into_iter().flatten() {
        let f = f.replace(['\'', '"', ';', '\\'], "");
        if !f.trim().is_empty() {
            chain.push('\'');
            chain.push_str(f.trim());
            chain.push_str("', ");
        }
    }
    chain.push_str(generic_family(panose));
    out.push(("font-family", chain));
    out.push(("font-size", mp(drawn)));
    if let AttrValue::Bold(true) = a.get(AttrSlot::TxtBold) {
        out.push(("font-weight", "bold".into()));
    }
    if let AttrValue::Italic(true) = a.get(AttrSlot::TxtItalic) {
        out.push(("font-style", "italic".into()));
    }
    if let AttrValue::Underline(true) = a.get(AttrSlot::TxtUnderline) {
        out.push(("text-decoration", "underline".into()));
    }
    // The twins.
    // The first name of `font-family` is the family whenever the chain can
    // spell it; otherwise the twin says it.
    if family.trim().is_empty()
        || family.contains(['\'', '"', ';', '\\', ','])
        || family.trim() != &*family
    {
        out.push(("xarast:family", family.to_string()));
    }
    if *family != *full {
        out.push(("xarast:font", full.to_string()));
    }
    if let Some(p) = panose {
        let hex: String = p.iter().map(|b| format!("{b:02x}")).collect();
        out.push(("xarast:panose", hex));
    }
    if let Some(s) = &substitute {
        out.push(("xarast:font-substitute", s.to_string()));
    }
    if drawn != size {
        out.push(("xarast:size", mp(size)));
    }
    if let AttrValue::AspectRatio(x) = a.get(AttrSlot::TxtAspectRatio)
        && x.to_bits() != 1.0f32.to_bits()
    {
        out.push(("xarast:aspect", f32s(*x)));
    }
    if let AttrValue::Tracking(t) = a.get(AttrSlot::TxtTracking)
        && t.raw() != 0
    {
        // Thousandths of an em, not millipoints (`docs/memory/text.md`).
        out.push(("xarast:tracking", t.raw().to_string()));
    }
    if script != xarast_doc::Script::default() {
        out.push((
            "xarast:script",
            format!(
                "{} {} {}",
                if script.on { "on" } else { "off" },
                f32s(script.offset),
                f32s(script.size)
            ),
        ));
    }
    if let AttrValue::Baseline(b) = a.get(AttrSlot::TxtBaseline)
        && b.raw() != 0
    {
        out.push(("xarast:baseline", mp(i64::from(b.raw()))));
    }
    match a.get(AttrSlot::TxtJustification) {
        AttrValue::Justification(xarast_doc::Justification::Centre) => {
            out.push(("xarast:justify", "centre".into()));
        }
        AttrValue::Justification(xarast_doc::Justification::Right) => {
            out.push(("xarast:justify", "right".into()));
        }
        AttrValue::Justification(xarast_doc::Justification::Full) => {
            out.push(("xarast:justify", "full".into()));
        }
        _ => {}
    }
    match a.get(AttrSlot::TxtLineSpace) {
        AttrValue::LineSpace(xarast_doc::LineSpacing::Ratio(r))
            if r.to_bits() != 1.0f32.to_bits() =>
        {
            out.push(("xarast:line-spacing", format!("ratio:{}", f32s(*r))));
        }
        AttrValue::LineSpace(xarast_doc::LineSpacing::Absolute(v)) => {
            out.push((
                "xarast:line-spacing",
                format!("abs:{}", mp(i64::from(v.raw()))),
            ));
        }
        _ => {}
    }
    for (slot, name) in [
        (AttrSlot::TxtLeftMargin, "xarast:indent-left"),
        (AttrSlot::TxtRightMargin, "xarast:indent-right"),
        (AttrSlot::TxtFirstIndent, "xarast:indent-first"),
    ] {
        if let AttrValue::LeftMargin(v) | AttrValue::RightMargin(v) | AttrValue::FirstIndent(v) =
            a.get(slot)
            && v.raw() != 0
        {
            out.push((name, mp(i64::from(v.raw()))));
        }
    }
    if let AttrValue::Ruler(r) = a.get(AttrSlot::TxtRuler)
        && !r.is_empty()
    {
        out.push(("xarast:ruler", ruler_text(r)));
    }
    out
}
