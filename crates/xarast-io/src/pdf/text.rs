//! Text as text (T11.4.7): the scene's text runs matched to their fill
//! ops, one embedded subset font per face, and the glyphs each run draws.
//!
//! The scene only holds outlines. [`SceneText`] (filled by the
//! application's walker) says which outline paths are text runs and which
//! glyphs they are made of; a run is found from its op by the identity of
//! its path allocation. Before the page is translated, every face the
//! runs use is subset **once** with every glyph any run draws from it
//! (`xarast_text::FontDb::pdf_font`) and written as a `Type0`/`CIDFont`
//! with a `ToUnicode` map built from the runs' cluster text. A face whose
//! licence forbids embedding (or that cannot be subset) gets no font: its
//! runs stay outlines and the report says so (`FontNotEmbedded`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use kurbo::{Affine, BezPath};
use xarast_render::PathRef;
use xarast_text::{EmbedError, FaceId};

use super::writer::{FontSpec, PdfWriter, Resource};
use crate::report::Compromise;
use crate::source::SceneText;

/// One glyph ready to show: its font, its CID and the matrix from its em
/// square to the run path's space.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PdfGlyph {
    pub font: Resource,
    pub cid: u16,
    pub em: Affine,
}

/// How a run can be drawn.
#[derive(Debug, Clone)]
pub(crate) struct RunPlan {
    /// The glyphs that have an embedded font, in layout order.
    pub glyphs: Vec<PdfGlyph>,
    /// Every glyph has an embedded font and is drawn at the face's default
    /// instance: the run can be *painted* as text. Otherwise it is painted
    /// as outlines, with the embedded glyphs as invisible text over them.
    pub native: bool,
    /// What the path holds besides the glyphs (the underline).
    pub decoration: Option<Arc<BezPath>>,
}

/// The text of one export.
#[derive(Debug, Default)]
pub(crate) struct PdfText {
    by_path: HashMap<usize, usize>,
    pub plans: Vec<RunPlan>,
    /// Runs already given their text (painted or invisible).
    pub done: Vec<bool>,
}

fn key(path: &PathRef) -> usize {
    std::ptr::from_ref(path.path()) as usize
}

/// A subset tag: six capital letters from the program's bytes (FNV-1a),
/// so a different subset gets a different name and the same one the same.
fn subset_tag(program: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in program {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    (0..6)
        .map(|_| {
            let c = char::from(b'A' + (h % 26) as u8);
            h /= 26;
            c
        })
        .collect()
}

impl PdfText {
    /// Subsets and writes every face the runs use; plans every run.
    pub(crate) fn new(
        text: &SceneText,
        w: &mut PdfWriter,
        compromises: &mut Vec<Compromise>,
    ) -> PdfText {
        let db = &text.fonts;
        // Faces in order of first use, so the file does not depend on how
        // the database numbered them.
        let mut order: Vec<FaceId> = Vec::new();
        let mut glyphs: HashMap<FaceId, BTreeSet<u32>> = HashMap::new();
        let mut texts: HashMap<FaceId, BTreeMap<u32, Arc<str>>> = HashMap::new();
        for run in &text.runs {
            for g in run.glyphs.iter() {
                let set = glyphs.entry(g.face).or_insert_with(|| {
                    order.push(g.face);
                    BTreeSet::new()
                });
                set.insert(g.id);
                if !g.text.is_empty() {
                    texts
                        .entry(g.face)
                        .or_default()
                        .entry(g.id)
                        .or_insert_with(|| Arc::clone(&g.text));
                }
            }
        }
        // Per face: its font and old glyph id → CID, plus the em scale.
        let mut fonts: HashMap<FaceId, (Resource, BTreeMap<u32, u16>, f64)> = HashMap::new();
        for face in order {
            let wanted = glyphs.remove(&face).unwrap_or_default();
            match db.pdf_font(face, wanted.iter().copied()) {
                Ok(f) => {
                    let mut to_unicode = BTreeMap::new();
                    if let Some(t) = texts.get(&face) {
                        for (old, s) in t {
                            if let Some(cid) = f.glyphs.get(old) {
                                to_unicode.insert(*cid, s.to_string());
                            }
                        }
                    }
                    let tag = subset_tag(&f.program);
                    let res = w.font(&FontSpec {
                        font: &f,
                        tag: &tag,
                        to_unicode: &to_unicode,
                    });
                    let upem = f64::from(db.units_per_em(face));
                    fonts.insert(face, (res, f.glyphs, upem));
                }
                Err(e) => {
                    let family = db
                        .face_info(face)
                        .map_or_else(|| Arc::from("(unnamed)"), |i| Arc::clone(&i.family));
                    let reason = match e {
                        EmbedError::Denied(_) => {
                            format!("{e}; its text is drawn as outlines")
                        }
                        other => format!("{other}; its text is drawn as outlines"),
                    };
                    let c = Compromise::FontNotEmbedded {
                        family,
                        reason: reason.into(),
                    };
                    if !compromises.contains(&c) {
                        compromises.push(c);
                    }
                }
            }
        }
        let mut out = PdfText::default();
        for (i, run) in text.runs.iter().enumerate() {
            let mut native = !run.glyphs.is_empty();
            let mut gl = Vec::with_capacity(run.glyphs.len());
            for g in run.glyphs.iter() {
                let Some((font, cids, upem)) = fonts.get(&g.face) else {
                    native = false;
                    continue;
                };
                let Some(cid) = cids.get(&g.id) else {
                    native = false;
                    continue;
                };
                native &= !g.variable;
                gl.push(PdfGlyph {
                    font: *font,
                    cid: *cid,
                    em: g.transform * Affine::scale(*upem),
                });
            }
            out.by_path.insert(key(&run.path), i);
            out.plans.push(RunPlan {
                glyphs: gl,
                native,
                decoration: run.decoration.clone(),
            });
        }
        out.done = vec![false; out.plans.len()];
        out
    }

    /// The run a fill or stroke op's path belongs to.
    pub(crate) fn run_of(&self, path: &PathRef) -> Option<usize> {
        self.by_path.get(&key(path)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_tags_are_six_capitals_and_follow_the_bytes() {
        let a = subset_tag(b"one");
        assert_eq!(a.len(), 6);
        assert!(a.chars().all(|c| c.is_ascii_uppercase()));
        assert_eq!(a, subset_tag(b"one"));
        assert_ne!(a, subset_tag(b"two"));
    }
}
