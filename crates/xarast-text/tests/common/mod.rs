//! The pinned test font set (`tests/fonts/`, see `PROVENANCE.md`), loaded
//! into an isolated database that never sees the system's fonts.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use xarast_geom::Mp;
use xarast_text::{
    FontDb, FontQuery, GenericName, ParagraphStyle, ScriptTag, Shaper, StoryInput, StoryMode,
    StyleRange,
};

pub fn font_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fonts")
        .join(name)
}

pub fn font_bytes(name: &str) -> Vec<u8> {
    std::fs::read(font_path(name)).expect("test font present")
}

pub const LATIN: &str = "NotoSans-Regular.subset.ttf";
pub const LATIN_BOLD: &str = "NotoSans-Bold.subset.ttf";
pub const LATIN_ITALIC: &str = "NotoSans-Italic.subset.ttf";
pub const HEBREW: &str = "NotoSansHebrew-Regular.subset.ttf";
pub const ARABIC: &str = "NotoSansArabic-Regular.subset.ttf";
pub const CJK: &str = "NotoSansCJKjp-Regular.subset.otf";
pub const VARIABLE: &str = "XarastTestVariable.ttf";

/// An isolated database holding the whole pinned set, with `sans-serif`
/// pointing at Noto Sans and a script fallback chain for Hebrew, Arabic and
/// CJK.
pub fn pinned_db() -> Arc<FontDb> {
    let db = FontDb::new_isolated();
    for f in [
        LATIN,
        LATIN_BOLD,
        LATIN_ITALIC,
        HEBREW,
        ARABIC,
        CJK,
        VARIABLE,
    ] {
        db.register_embedded(None, font_bytes(f))
            .expect("fixture parses");
    }
    assert_eq!(
        db.set_generic_families(GenericName::SansSerif, &["Noto Sans"]),
        1
    );
    assert_eq!(
        db.set_fallback_preference(ScriptTag::HEBREW, &["Noto Sans Hebrew"]),
        1
    );
    assert_eq!(
        db.set_fallback_preference(ScriptTag::ARABIC, &["Noto Sans Arabic"]),
        1
    );
    for s in [ScriptTag::HAN, ScriptTag::HIRAGANA, ScriptTag::KATAKANA] {
        assert_eq!(db.set_fallback_preference(s, &["Noto Sans CJK JP"]), 1);
    }
    Arc::new(db)
}

pub fn shaper() -> Shaper {
    Shaper::new(pinned_db())
}

/// One run of `family` at `size_pt` over the whole text.
pub fn run(text: &str, family: &str, size_pt: i32) -> StyleRange {
    StyleRange::new(
        0..text.len(),
        FontQuery::new(family),
        Mp::new(size_pt * 1000),
    )
}

pub fn lay(
    shaper: &Shaper,
    text: &str,
    runs: &[StyleRange],
    para: ParagraphStyle,
    mode: StoryMode,
) -> xarast_text::Layout {
    shaper.layout(&StoryInput {
        text,
        runs,
        paragraphs: &[para],
        kerns: &[],
        mode,
    })
}
