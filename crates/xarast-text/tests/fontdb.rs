//! W9.1 — the font database over the pinned test set. No system fonts.

mod common;

use common::*;
use xarast_text::{
    FontDb, FontError, FontQuery, FontStyle, GenericName, ScriptTag, SubstitutionReason,
};

#[test]
fn families_are_enumerated_sorted_and_deduplicated() {
    let db = pinned_db();
    let fams: Vec<String> = db.families().iter().map(|f| f.to_string()).collect();
    assert_eq!(
        fams,
        [
            "Noto Sans",
            "Noto Sans Arabic",
            "Noto Sans CJK JP",
            "Noto Sans Hebrew",
            "Xarast Test Variable"
        ]
    );
}

#[test]
fn query_matches_weight_and_style_within_a_family() {
    let db = pinned_db();
    let regular = db.query(&FontQuery::new("Noto Sans")).unwrap();
    let bold = db
        .query(&FontQuery::new("Noto Sans").with_weight(700))
        .unwrap();
    let italic = db
        .query(&FontQuery::new("Noto Sans").with_style(FontStyle::Italic))
        .unwrap();
    assert!(regular.substitution.is_none());
    assert_ne!(regular.face, bold.face);
    assert_ne!(regular.face, italic.face);
    assert_eq!(db.face_info(bold.face).unwrap().weight, 700);
    assert_eq!(db.face_info(italic.face).unwrap().style, FontStyle::Italic);
    // Semibold has no face of its own: the nearest heavier face wins (CSS
    // font matching), and nothing is synthesised for an exact-enough match.
    let semi = db
        .query(&FontQuery::new("Noto Sans").with_weight(600))
        .unwrap();
    assert_eq!(semi.face, bold.face);
    // Bold italic has no face: the italic is chosen and bolded synthetically.
    let bi = db
        .query(
            &FontQuery::new("Noto Sans")
                .with_weight(700)
                .with_style(FontStyle::Italic),
        )
        .unwrap();
    assert!(bi.synthesis.embolden || bi.face == bold.face);
}

#[test]
fn names_are_matched_case_and_whitespace_insensitively() {
    let db = pinned_db();
    let a = db.query(&FontQuery::new("noto   SANS ")).unwrap();
    let b = db.query(&FontQuery::new("Noto Sans")).unwrap();
    assert_eq!(a.face, b.face);
    assert!(a.substitution.is_none());
}

#[test]
fn a_style_suffix_is_stripped_and_honoured() {
    let db = pinned_db();
    let m = db.query(&FontQuery::new("Noto Sans Bold")).unwrap();
    let s = m.substitution.expect("recorded as a substitution");
    assert_eq!(s.reason, SubstitutionReason::StyleSuffix);
    assert_eq!(&*s.requested, "Noto Sans Bold");
    assert_eq!(db.face_info(m.face).unwrap().weight, 700);
}

#[test]
fn a_metric_alias_is_preferred_over_the_generic_family() {
    let db = FontDb::new_isolated();
    // The Noto face registered under an alias member's name stands in for an
    // installed Liberation Sans.
    db.register_embedded(Some("Liberation Sans"), font_bytes(LATIN))
        .unwrap();
    db.register_embedded(None, font_bytes(HEBREW)).unwrap();
    db.set_generic_families(GenericName::SansSerif, &["Noto Sans Hebrew"]);
    let m = db.query(&FontQuery::new("Arial")).unwrap();
    assert_eq!(&*m.family, "Liberation Sans");
    assert_eq!(
        m.substitution.unwrap().reason,
        SubstitutionReason::MetricAlias
    );
    assert_eq!(db.substitutions().len(), 1);
}

#[test]
fn a_missing_family_falls_to_its_generic_then_to_the_last_resort() {
    let db = pinned_db();
    let m = db.query(&FontQuery::new("Frutiger")).unwrap();
    assert_eq!(&*m.family, "Noto Sans");
    assert_eq!(m.substitution.unwrap().reason, SubstitutionReason::Generic);
    // No monospace generic is configured: the ladder ends at sans-serif.
    let m = db.query(&FontQuery::new("Courier New")).unwrap();
    assert_eq!(&*m.family, "Noto Sans");
    assert_eq!(
        m.substitution.unwrap().reason,
        SubstitutionReason::LastResort
    );
    // The document's name is kept for the report, never replaced.
    let subs = db.substitutions();
    assert!(subs.iter().any(|s| &*s.requested == "Frutiger"));
    assert!(subs.iter().any(|s| &*s.requested == "Courier New"));
}

#[test]
fn panose_picks_the_generic_family_of_a_missing_face() {
    let db = FontDb::new_isolated();
    db.register_embedded(None, font_bytes(LATIN)).unwrap();
    db.register_embedded(Some("Serif Stand-in"), font_bytes(LATIN_ITALIC))
        .unwrap();
    db.set_generic_families(GenericName::SansSerif, &["Noto Sans"]);
    db.set_generic_families(GenericName::Serif, &["Serif Stand-in"]);
    let serif_panose = [2, 2, 6, 3, 5, 4, 5, 2, 3, 4];
    let m = db
        .query_with_panose(&FontQuery::new("Mystery Face"), Some(serif_panose))
        .unwrap();
    assert_eq!(&*m.family, "Serif Stand-in");
    let m = db.query(&FontQuery::new("Mystery Face")).unwrap();
    assert_eq!(&*m.family, "Noto Sans");
}

#[test]
fn an_empty_database_matches_nothing() {
    let db = FontDb::new_isolated();
    assert!(db.query(&FontQuery::new("Anything")).is_none());
    assert!(db.fallback_for('a', &FontQuery::new("Anything")).is_none());
    assert!(db.families().is_empty());
}

#[test]
fn fallback_follows_the_script_chain() {
    let db = pinned_db();
    let base = FontQuery::new("Noto Sans");
    let fam = |c| {
        let id = db.fallback_for(c, &base).unwrap();
        db.face_info(id).unwrap().family.to_string()
    };
    assert_eq!(fam('ש'), "Noto Sans Hebrew");
    assert_eq!(fam('ب'), "Noto Sans Arabic");
    assert_eq!(fam('漢'), "Noto Sans CJK JP");
    assert_eq!(fam('の'), "Noto Sans CJK JP");
    // No chain for Latin: the generic families are the last stop.
    assert_eq!(fam('a'), "Noto Sans");
    // Nothing covers it at all.
    assert!(
        db.fallback_for('\u{0E01}', &base).is_none(),
        "Thai is not in the set"
    );
}

#[test]
fn the_fallback_preference_overrides_the_chain() {
    let db = pinned_db();
    let base = FontQuery::new("Noto Sans");
    // U+0020 is Common: every face covers it, so the preference alone decides.
    db.set_fallback_preference(ScriptTag::COMMON, &["Noto Sans Hebrew", "Noto Sans"]);
    let f = db.fallback_for(' ', &base).unwrap();
    assert_eq!(&*db.face_info(f).unwrap().family, "Noto Sans Hebrew");
    db.set_fallback_preference(ScriptTag::COMMON, &["Noto Sans CJK JP"]);
    let f = db.fallback_for(' ', &base).unwrap();
    assert_eq!(&*db.face_info(f).unwrap().family, "Noto Sans CJK JP");
    // A preferred family that does not cover the character is skipped.
    db.set_fallback_preference(ScriptTag::HEBREW, &["Noto Sans Arabic", "Noto Sans Hebrew"]);
    let f = db.fallback_for('ש', &base).unwrap();
    assert_eq!(&*db.face_info(f).unwrap().family, "Noto Sans Hebrew");
    // Unknown names are dropped from the preference, not fatal.
    assert_eq!(
        db.set_fallback_preference(ScriptTag::HEBREW, &["No Such Face"]),
        0
    );
}

#[test]
fn face_data_is_shared_not_copied() {
    let db = pinned_db();
    let id = db.query(&FontQuery::new("Noto Sans")).unwrap().face;
    let a = db.face_data(id).unwrap();
    let b = db.face_data(id).unwrap();
    assert_eq!(a.bytes().as_ptr(), b.bytes().as_ptr());
    assert_eq!(a.bytes(), font_bytes(LATIN).as_slice());
    assert_eq!(a.index(), 0);
}

#[test]
fn embedded_faces_are_flagged_and_keep_their_bytes() {
    let db = FontDb::new_isolated();
    let ids = db
        .register_embedded(Some("Document Face"), font_bytes(LATIN_BOLD))
        .unwrap();
    assert_eq!(ids.len(), 1);
    let info = db.face_info(ids[0]).unwrap();
    assert!(info.embedded);
    assert_eq!(&*info.family, "Document Face");
    let m = db.query(&FontQuery::new("document face")).unwrap();
    assert_eq!(m.face, ids[0]);
    assert!(m.embedded);
}

#[test]
fn registering_under_an_existing_family_name_extends_that_family() {
    // Two registered faces under one name form one family; shadowing of a
    // *system* family by a registered one is asserted in `system_fonts.rs`,
    // which needs the host's fonts.
    let db = FontDb::new_isolated();
    db.register_embedded(None, font_bytes(LATIN)).unwrap();
    let before = db.query(&FontQuery::new("Noto Sans")).unwrap();
    let ids = db
        .register_embedded(Some("Noto Sans"), font_bytes(LATIN_ITALIC))
        .unwrap();
    let after = db
        .query(&FontQuery::new("Noto Sans").with_style(FontStyle::Italic))
        .unwrap();
    assert_eq!(after.face, ids[0]);
    assert_ne!(before.face, after.face);
}

#[test]
fn unreadable_bytes_are_rejected() {
    let db = FontDb::new_isolated();
    assert_eq!(
        db.register_embedded(Some("Junk"), b"not a font at all".to_vec()),
        Err(FontError::Unreadable)
    );
    assert_eq!(
        db.register_embedded(Some("Empty"), Vec::new()),
        Err(FontError::Unreadable)
    );
}

/// Rewrites `OS/2.fsType` of a TrueType file in place.
fn with_fs_type(mut bytes: Vec<u8>, fs_type: u16) -> Vec<u8> {
    let n = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    for i in 0..n {
        let rec = 12 + 16 * i;
        if &bytes[rec..rec + 4] == b"OS/2" {
            let off = u32::from_be_bytes(bytes[rec + 8..rec + 12].try_into().unwrap()) as usize;
            bytes[off + 8..off + 10].copy_from_slice(&fs_type.to_be_bytes());
            return bytes;
        }
    }
    panic!("no OS/2 table");
}

#[test]
fn fs_type_decides_whether_a_face_may_be_embedded() {
    let db = FontDb::new_isolated();
    let open = db
        .register_embedded(Some("Open"), font_bytes(LATIN))
        .unwrap()[0];
    let restricted = db
        .register_embedded(Some("Restricted"), with_fs_type(font_bytes(LATIN), 0x0002))
        .unwrap()[0];
    let print = db
        .register_embedded(Some("Print"), with_fs_type(font_bytes(LATIN), 0x0004))
        .unwrap()[0];
    let bitmap_only = db
        .register_embedded(Some("Bitmap"), with_fs_type(font_bytes(LATIN), 0x0208))
        .unwrap()[0];
    assert!(!db.embedding_denied(open));
    assert!(db.embedding_denied(restricted));
    assert!(!db.embedding_denied(print));
    assert!(db.embedding_denied(bitmap_only));
}
