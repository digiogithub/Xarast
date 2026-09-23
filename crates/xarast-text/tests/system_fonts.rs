//! Opt-in tests against the machine's installed fonts (T9.1.8's Linux smoke
//! test). They are skipped unless `XARAST_SYSTEM_FONT_TESTS=1`: a result that
//! depends on the host's font set is not deterministic.

mod common;

use std::time::Instant;

use common::*;
use xarast_geom::Mp;
use xarast_text::{FontDb, FontDbOptions, FontQuery, ParagraphStyle, Shaper, StoryMode};

fn enabled() -> bool {
    let on = std::env::var_os("XARAST_SYSTEM_FONT_TESTS").is_some_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set XARAST_SYSTEM_FONT_TESTS=1 to run against system fonts");
    }
    on
}

#[test]
fn system_fonts_enumerate_and_match() {
    if !enabled() {
        return;
    }
    let db = FontDb::new_system();
    assert!(db.families().is_empty(), "enumeration is deferred");
    let t = Instant::now();
    let n = db.load_system_fonts();
    eprintln!("enumerated {n} families in {:?}", t.elapsed());
    assert!(n > 0, "a desktop has fonts");
    // fontconfig lists families fontique cannot open (Type 1 `.pfb`, for
    // one); those go down the substitution ladder like a missing font.
    let mut unmatched = Vec::new();
    let fams = db.families();
    for fam in fams.iter().take(200) {
        let m = db.query(&FontQuery::new(fam)).expect("something matches");
        assert!(!m.embedded);
        assert!(db.face_data(m.face).is_some(), "{fam} loads");
        if m.substitution.is_some() {
            unmatched.push(fam.clone());
        }
    }
    eprintln!("listed but not openable: {unmatched:?}");
    assert!(
        unmatched.len() * 10 < fams.len().min(200),
        "most families open"
    );
    // Something always renders Latin text.
    let shaper = Shaper::new(std::sync::Arc::new(db));
    let text = "Hello, world";
    let l = lay(
        &shaper,
        text,
        &[run(text, "sans-serif-that-does-not-exist", 12)],
        ParagraphStyle::default(),
        StoryMode::Point,
    );
    assert!(l.glyphs().all(|(_, g)| g.id != 0), "no .notdef for ASCII");
}

#[test]
fn an_embedded_face_shadows_the_installed_family_of_the_same_name() {
    if !enabled() {
        return;
    }
    let db = FontDb::new_system();
    db.load_system_fonts();
    // A system family with a real bold face.
    let fams = db.families();
    let Some((family, sys_bold)) = fams.iter().find_map(|f| {
        let m = db.query(&FontQuery::new(f).with_weight(700))?;
        let info = db.face_info(m.face)?;
        (m.substitution.is_none() && info.weight == 700 && !m.synthesis.embolden)
            .then(|| (f.clone(), m.face))
    }) else {
        eprintln!("no system family with a bold face; nothing to shadow");
        return;
    };
    let ids = db
        .register_embedded(Some(&family), font_bytes(LATIN))
        .unwrap();
    let after = db.query(&FontQuery::new(&family).with_weight(700)).unwrap();
    assert_eq!(after.face, ids[0], "{family}: the document's face wins");
    assert_ne!(after.face, sys_bold);
    assert!(after.embedded);
    assert!(
        after.synthesis.embolden,
        "the regular embedded face is bolded"
    );
}

#[test]
fn the_face_cache_is_bounded() {
    if !enabled() {
        return;
    }
    let db = FontDb::with_options(FontDbOptions {
        system_fonts: true,
        face_cache_capacity: 2,
    });
    let fams = db.families();
    let faces: Vec<_> = fams
        .iter()
        .take(6)
        .filter_map(|f| db.query(&FontQuery::new(f)).map(|m| m.face))
        .collect();
    for &f in &faces {
        assert!(db.face_data(f).is_some());
        assert!(db.resident_system_faces() <= 2);
    }
    // An evicted face reloads.
    assert!(db.face_data(faces[0]).is_some());
    let _ = Mp::ZERO;
}
