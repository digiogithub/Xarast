//! W9.3 shaping against the pinned faces: golden glyph sequences, kerning,
//! complex scripts, bidi and combining marks.

mod common;

use common::*;
use xarast_geom::Mp;
use xarast_text::{
    Direction, FontFeature, FontMetrics, FontQuery, FontVariation, Layout, ParagraphStyle,
    StoryMode, StyleRange,
};

fn point(text: &str, family: &str) -> Layout {
    lay(
        &shaper(),
        text,
        &[run(text, family, 10)],
        ParagraphStyle::default(),
        StoryMode::Point,
    )
}

/// `(glyph id, x)` for the first line, left to right.
fn ids_x(l: &Layout) -> Vec<(u32, i32)> {
    l.lines[0]
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .map(|g| (g.id, g.x.raw()))
        .collect()
}

/// Cluster start offsets of the first line, left to right.
fn visual_clusters(l: &Layout) -> Vec<usize> {
    let mut c: Vec<_> = l.lines[0].clusters.iter().collect();
    c.sort_by_key(|c| c.x);
    c.iter().map(|c| c.range.start).collect()
}

// ── Golden sequences (acceptance criterion 1) ──────────────────────────────
//
// Pinned to the fixture bytes in `tests/fonts/` (SHA-256 in PROVENANCE.md)
// and to harfrust's shaping. A change here means the shaper changed.

#[test]
fn golden_latin_with_kerning_and_a_ligature() {
    let l = point("AVATAR office", "Noto Sans");
    assert_eq!(
        ids_x(&l),
        [
            (34, 0),
            (55, 5990),
            (34, 11590),
            (53, 17280),
            (34, 22140),
            (51, 28530),
            (1, 34750),
            (80, 37350),
            (260, 43400), // "ffi", one glyph for three characters
            (68, 52860),
            (70, 57660)
        ]
    );
    let ffi = &l.lines[0].clusters[8];
    assert_eq!(ffi.range, 8..11, "a ligature is one cluster");
}

#[test]
fn golden_hebrew() {
    let l = point("שלום עולם", "Noto Sans Hebrew");
    assert_eq!(
        ids_x(&l),
        [
            (21, 0),
            (48, 6840),
            (110, 12060),
            (8, 15070),
            (97, 21000), // the Hebrew face's own space
            (21, 23700),
            (110, 30440),
            (48, 33450),
            (87, 38670)
        ]
    );
}

#[test]
fn golden_arabic_joins_and_ligates() {
    let l = point("لا سلام", "Noto Sans Arabic");
    assert_eq!(
        ids_x(&l),
        [
            (326, 0),
            (276, 4840),
            (449, 10830),
            (1, 18670),
            (275, 21270)
        ]
    );
    // Lam-alef is one ligature glyph and one cluster, in both words.
    let clusters: Vec<_> = l.lines[0]
        .clusters
        .iter()
        .map(|c| c.range.clone())
        .collect();
    assert_eq!(clusters, [0..4, 4..5, 5..7, 7..11, 11..13]);
    // Initial seen (449, joined to the lam after it) differs from the
    // isolated seen: joining happened.
    let alone = point("س", "Noto Sans Arabic");
    assert_ne!(ids_x(&alone)[0].0, 449);
}

#[test]
fn golden_cjk() {
    let l = point("日本語", "Noto Sans CJK JP");
    assert_eq!(ids_x(&l), [(33, 0), (34, 10000), (44, 20000)]);
}

// ── Kerning ─────────────────────────────────────────────────────────────────

#[test]
fn auto_kern_can_be_turned_off() {
    let s = shaper();
    let text = "AV";
    let r = [run(text, "Noto Sans", 10)];
    let on = lay(&s, text, &r, ParagraphStyle::default(), StoryMode::Point);
    let off = lay(
        &s,
        text,
        &r,
        ParagraphStyle {
            auto_kern: false,
            ..ParagraphStyle::default()
        },
        StoryMode::Point,
    );
    assert_eq!(ids_x(&on)[1].1, 5990);
    assert_eq!(
        ids_x(&off)[1].1,
        6390,
        "A's plain advance, 639 units at 10 pt"
    );
    let k = s.kern_pair(&FontQuery::new("Noto Sans"), Mp::new(10_000), 'A', 'V');
    assert_eq!(k, Mp::new(-400));
    // A feature list can turn it off per run too.
    let mut r0 = run(text, "Noto Sans", 10);
    r0.features = vec![FontFeature {
        tag: *b"kern",
        value: 0,
    }]
    .into();
    let feat = lay(&s, text, &[r0], ParagraphStyle::default(), StoryMode::Point);
    assert_eq!(ids_x(&feat)[1].1, 6390);
}

#[test]
fn liga_can_be_turned_off() {
    let s = shaper();
    let text = "office";
    let mut r0 = run(text, "Noto Sans", 10);
    r0.features = vec![FontFeature {
        tag: *b"liga",
        value: 0,
    }]
    .into();
    let l = lay(&s, text, &[r0], ParagraphStyle::default(), StoryMode::Point);
    assert_eq!(l.lines[0].clusters.len(), 6, "no ligature, six clusters");
}

// ── Combining marks ─────────────────────────────────────────────────────────

#[test]
fn a_base_and_its_marks_are_one_cluster_with_the_mark_over_the_base() {
    let s = shaper();
    let text = "xa\u{0308}\u{0323}e\u{0301}";
    let l = lay(
        &s,
        text,
        &[run(text, "Noto Sans", 10)],
        ParagraphStyle::default(),
        StoryMode::Point,
    );
    let ranges: Vec<_> = l.lines[0]
        .clusters
        .iter()
        .map(|c| c.range.clone())
        .collect();
    assert_eq!(ranges, [0..1, 1..6, 6..9]);
    // The dot below is drawn inside its base's box.
    let base = &l.lines[0].clusters[1];
    let db = s.fonts();
    let (run, mark) = l
        .glyphs()
        .filter(|(_, g)| g.cluster == 1)
        .last()
        .expect("the cluster has glyphs");
    let upem = db.units_per_em(run.face);
    let path = db
        .glyph_outline_normalized(run.face, mark.id, &run.coords)
        .unwrap();
    let bbox = kurbo::Shape::bounding_box(&(run.glyph_transform(mark, upem) * (*path).clone()));
    let centre = (bbox.x0 + bbox.x1) / 2.0;
    assert!(
        centre > base.x.to_f64() && centre < (base.x + base.width).to_f64(),
        "mark centre {centre} inside base box {}..{}",
        base.x.raw(),
        (base.x + base.width).raw()
    );
    assert!(bbox.y1 < 0.0, "the dot below is below the baseline");
}

#[test]
fn a_pathological_combining_sequence_terminates() {
    let s = shaper();
    let mut text = String::from("a");
    text.extend(std::iter::repeat_n('\u{0301}', 2000));
    let l = lay(
        &s,
        &text,
        &[run(&text, "Noto Sans", 10)],
        ParagraphStyle::default(),
        StoryMode::column(Mp::new(50_000)),
    );
    assert_eq!(l.lines.len(), 1);
    assert_eq!(l.lines[0].clusters.len(), 1, "never split");
}

// ── Bidi (acceptance criterion 4, on the pinned Hebrew face) ────────────────

#[test]
fn rtl_leftmost_glyph_has_the_highest_cluster_index() {
    let l = point("שלום עולם", "Noto Sans Hebrew");
    let first = l.lines[0].runs[0].glyphs[0];
    let max = l.lines[0]
        .clusters
        .iter()
        .map(|c| c.range.start)
        .max()
        .unwrap();
    assert_eq!(first.cluster, max);
}

#[test]
fn numbers_inside_rtl_text_stay_left_to_right() {
    let l = point("abc שלום 123 def", "Noto Sans");
    // abc␣ 123 ␣ םולש ␣def: the Hebrew run is reversed as a whole, the
    // digits inside it keep their order (level 2).
    assert_eq!(
        visual_clusters(&l),
        [0, 1, 2, 3, 13, 14, 15, 12, 10, 8, 6, 4, 16, 17, 18, 19]
    );
    let levels: Vec<u8> = l.lines[0].runs.iter().map(|r| r.level).collect();
    assert!(levels.contains(&2));
}

#[test]
fn an_explicit_base_direction_places_neutrals() {
    let s = shaper();
    let text = "abc!";
    let r = [run(text, "Noto Sans", 10)];
    let rtl = lay(
        &s,
        text,
        &r,
        ParagraphStyle {
            base_direction: Direction::Rtl,
            ..ParagraphStyle::default()
        },
        StoryMode::Point,
    );
    assert_eq!(
        visual_clusters(&rtl),
        [3, 0, 1, 2],
        "'!' ends up on the left"
    );
    let ltr = lay(&s, text, &r, ParagraphStyle::default(), StoryMode::Point);
    assert_eq!(visual_clusters(&ltr), [0, 1, 2, 3]);
}

#[test]
fn a_mixed_script_story_has_no_notdef() {
    // Acceptance criterion 15, for the scripts in the pinned set.
    let text = "Xarast שלום سلام 日本語 e\u{301}";
    let l = point(text, "Noto Sans");
    assert!(l.glyphs().count() > 10);
    for (r, g) in l.glyphs() {
        assert_ne!(g.id, 0, "notdef at {} in face {:?}", g.cluster, r.face);
    }
    // And fallback put each script in its own face.
    let faces: std::collections::BTreeSet<_> = l.glyphs().map(|(r, _)| r.face).collect();
    assert_eq!(faces.len(), 4);
}

// ── Variations ──────────────────────────────────────────────────────────────

#[test]
fn variation_settings_reach_the_run() {
    let s = shaper();
    let text = "II";
    let mut r0 = run(text, "Xarast Test Variable", 10);
    r0.variations = vec![FontVariation {
        tag: *b"wght",
        value: 900.0,
    }]
    .into();
    let l = lay(&s, text, &[r0], ParagraphStyle::default(), StoryMode::Point);
    let r = &l.lines[0].runs[0];
    assert_eq!(&r.coords[..], &[16384], "wght 900 is +1.0 normalised");
    assert_eq!(
        ids_x(&l),
        [(2, 0), (2, 3000)],
        "advance is 300 units at any weight"
    );
}

// ── FontMetrics ─────────────────────────────────────────────────────────────

#[test]
fn char_metrics_scale_with_size_and_aspect() {
    let s = shaper();
    let q = FontQuery::new("Noto Sans");
    let m10 = s.char_metrics(&q, Mp::new(10_000), 1.0, 'A').unwrap();
    let m20 = s.char_metrics(&q, Mp::new(20_000), 1.0, 'A').unwrap();
    let wide = s.char_metrics(&q, Mp::new(10_000), 2.0, 'A').unwrap();
    assert_eq!(m10.advance, Mp::new(6390));
    assert_eq!(m20.advance, Mp::new(12780));
    assert_eq!(wide.advance, Mp::new(12780));
    // The width of Noto Sans' "M" (907 units) at 20 pt of stretched em.
    assert_eq!(wide.em_width, Mp::new(18_140));
    assert_eq!(m10.ascent, Mp::new(10690));
    assert_eq!(m10.descent, Mp::new(2930));
    assert_eq!(m10.glyph, 34);
    let _ = StyleRange::new(0..1, q, Mp::ZERO);
}
