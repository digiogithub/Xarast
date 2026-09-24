//! Font embedding (`xarast_text::embed`): WOFF2 web fonts and PDF font
//! programs from the pinned faces, and the `fsType` refusal.

mod common;

use common::*;
use skrifa::MetadataProvider;
use skrifa::outline::DrawSettings;
use skrifa::prelude::{LocationRef, Size};
use xarast_text::embed::{with_fs_type, woff2};
use xarast_text::{EmbedError, Embedding, FaceId, FontDb, FontQuery, ProgramFormat};

fn face(db: &FontDb, family: &str) -> FaceId {
    db.query(&FontQuery::new(family)).expect("face").face
}

/// A glyph's outline as SVG path data, in font units.
fn outline(font: &skrifa::FontRef<'_>, gid: u32) -> String {
    struct Pen(String);
    impl skrifa::outline::OutlinePen for Pen {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push_str(&format!("M{x} {y}"));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push_str(&format!("L{x} {y}"));
        }
        fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
            self.0.push_str(&format!("Q{a} {b} {x} {y}"));
        }
        fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
            self.0.push_str(&format!("C{a} {b} {c} {d} {x} {y}"));
        }
        fn close(&mut self) {
            self.0.push('Z');
        }
    }
    let mut pen = Pen(String::new());
    let g = font
        .outline_glyphs()
        .get(skrifa::GlyphId::new(gid))
        .expect("glyph");
    g.draw(
        DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
        &mut pen,
    )
    .expect("draws");
    pen.0
}

#[test]
fn a_web_font_maps_exactly_the_drawn_characters_to_the_same_outlines() {
    let db = pinned_db();
    for (family, file, text) in [
        ("Noto Sans", LATIN, "Hello, Xarast! fi"),
        ("Noto Sans CJK JP", CJK, "日本語"),
        ("Noto Sans Hebrew", HEBREW, "שלום"),
    ] {
        let id = face(&db, family);
        let chars: Vec<char> = text.chars().collect();
        let w = db.web_font(id, &chars).expect("embeds");
        assert!(!w.whole);
        assert_eq!(&w.woff2[0..4], b"wOF2");
        let ttf = woff2::decode(&w.woff2).expect("our WOFF2 decodes");
        let sub = skrifa::FontRef::new(&ttf).expect("the subset parses");
        let orig_bytes = font_bytes(file);
        let orig = skrifa::FontRef::new(&orig_bytes).unwrap();
        let mut mapped = 0;
        for c in text.chars() {
            let Some(og) = orig.charmap().map(c).filter(|g| g.to_u32() != 0) else {
                assert!(sub.charmap().map(c).is_none_or(|g| g.to_u32() == 0));
                continue;
            };
            let sg = sub.charmap().map(c).expect("mapped in the subset");
            assert_eq!(
                outline(&sub, sg.to_u32()),
                outline(&orig, og.to_u32()),
                "{family} {c:?}"
            );
            mapped += 1;
        }
        assert!(mapped > 0, "{family}");
        // A character nobody drew is not in the subset.
        assert!(sub.charmap().map('Q').is_none_or(|g| g.to_u32() == 0));
        // Far smaller than the face, and the same bytes every time.
        assert!(w.woff2.len() < orig_bytes.len(), "{family}");
        assert_eq!(db.web_font(id, &chars).unwrap().woff2, w.woff2);
        // The face's own OS/2 goes along.
        assert_eq!(
            skrifa::raw::TableProvider::os2(&sub).unwrap().fs_type(),
            skrifa::raw::TableProvider::os2(&orig).unwrap().fs_type()
        );
    }
}

#[test]
fn a_pdf_program_keeps_the_glyphs_asked_for_with_their_widths() {
    let db = pinned_db();
    // TrueType.
    let id = face(&db, "Noto Sans");
    let orig_bytes = font_bytes(LATIN);
    let orig = skrifa::FontRef::new(&orig_bytes).unwrap();
    let gids: Vec<u32> = "Xara"
        .chars()
        .map(|c| orig.charmap().map(c).unwrap().to_u32())
        .collect();
    let p = db.pdf_font(id, gids.iter().copied()).expect("embeds");
    assert_eq!(p.format, ProgramFormat::TrueType);
    assert_eq!(p.postscript_name, "NotoSans-Regular");
    let sub = skrifa::FontRef::new(&p.program).expect("a TrueType file");
    assert_eq!(p.glyphs.len(), 4, "3 letters + .notdef");
    assert_eq!(p.glyphs.get(&0), Some(&0));
    let upem = f32::from(
        orig.metrics(Size::unscaled(), LocationRef::default())
            .units_per_em,
    );
    let gm = orig.glyph_metrics(Size::unscaled(), LocationRef::default());
    for g in &gids {
        let new = p.glyphs[g];
        assert_eq!(outline(&sub, u32::from(new)), outline(&orig, *g));
        let w = gm.advance_width(skrifa::GlyphId::new(*g)).unwrap() * 1000.0 / upem;
        assert!((p.widths[usize::from(new)] - w).abs() < 1e-3);
    }
    assert!(p.ascent > 0.0 && p.descent < 0.0 && p.cap_height > 0.0);
    // CFF: the bare, CID-keyed CFF table.
    let id = face(&db, "Noto Sans CJK JP");
    let cjk = font_bytes(CJK);
    let cjk = skrifa::FontRef::new(&cjk).unwrap();
    let g = cjk.charmap().map('語').unwrap().to_u32();
    let p = db.pdf_font(id, [g]).expect("embeds");
    assert_eq!(p.format, ProgramFormat::Cff);
    assert_eq!(p.program[0], 1, "CFF major version 1");
    assert_eq!(p.glyphs.len(), 2);
    // Deterministic.
    assert_eq!(db.pdf_font(id, [g]).unwrap().program, p.program);
}

#[test]
fn a_face_whose_licence_forbids_embedding_is_refused_everywhere() {
    let db = FontDb::new_isolated();
    let restricted = with_fs_type(&font_bytes(LATIN), 0x0002).unwrap();
    let ids = db
        .register_embedded(Some("Restricted Sans"), restricted)
        .unwrap();
    let id = ids[0];
    assert_eq!(db.embed_rights(id).level, Embedding::Restricted);
    assert!(db.embedding_denied(id));
    assert_eq!(
        db.web_font(id, &['a']).unwrap_err(),
        EmbedError::Denied(Embedding::Restricted)
    );
    assert_eq!(
        db.pdf_font(id, [1]).unwrap_err(),
        EmbedError::Denied(Embedding::Restricted)
    );
    // Bitmap-only embedding is a refusal for outlines too.
    let bitmap = with_fs_type(&font_bytes(LATIN), 0x0200).unwrap();
    let id = db.register_embedded(Some("Bitmap Sans"), bitmap).unwrap()[0];
    assert!(db.embedding_denied(id));
    // Preview & print embedding is allowed: Xarast only draws the subset.
    let preview = with_fs_type(&font_bytes(LATIN), 0x0004).unwrap();
    let id = db.register_embedded(Some("Preview Sans"), preview).unwrap()[0];
    assert!(db.web_font(id, &['a']).is_ok());
}

#[test]
fn a_face_that_forbids_subsetting_is_embedded_whole() {
    let db = FontDb::new_isolated();
    let bytes = with_fs_type(&font_bytes(LATIN), 0x0100).unwrap();
    let id = db.register_embedded(Some("Whole Sans"), bytes).unwrap()[0];
    let w = db.web_font(id, &['a']).unwrap();
    assert!(w.whole);
    let ttf = woff2::decode(&w.woff2).unwrap();
    let sub = skrifa::FontRef::new(&ttf).unwrap();
    let orig_bytes = font_bytes(LATIN);
    let orig = skrifa::FontRef::new(&orig_bytes).unwrap();
    let count = |f: &skrifa::FontRef<'_>| {
        f.metrics(Size::unscaled(), LocationRef::default())
            .glyph_count
    };
    assert_eq!(count(&sub), count(&orig));
    assert_eq!(
        sub.charmap().mappings().count(),
        orig.charmap()
            .mappings()
            .filter(|(_, g)| g.to_u32() != 0)
            .count()
    );
    let p = db.pdf_font(id, [5]).unwrap();
    assert!(p.whole);
    assert_eq!(p.glyphs.len(), usize::from(count(&orig)));
}

#[test]
fn nothing_to_embed_is_an_error_not_an_empty_font() {
    let db = pinned_db();
    let id = face(&db, "Noto Sans Hebrew");
    assert_eq!(db.web_font(id, &['日']).unwrap_err(), EmbedError::Empty);
    assert_eq!(db.web_font(id, &[]).unwrap_err(), EmbedError::Empty);
}
