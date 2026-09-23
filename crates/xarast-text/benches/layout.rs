//! Shaping and layout budgets of `docs/phases/phase-09-text.md`.
//!
//! `cargo bench -p xarast-text --bench layout`
//!
//! * `paragraph_1k` — shape and lay out a ~1 000-glyph paragraph into a
//!   column, fully justified. Budget: ≤ 8 ms.
//! * `story_10k` — a 10 000-character story in 20 paragraphs, mixed Latin
//!   and Hebrew, wrapped and justified. Budget: ≤ 60 ms.
//! * `outline_cached` — one cached glyph outline lookup. Budget: ≤ 500 ns.
//!
//! "Cold" in the phase document means no layout cache (there is none yet,
//! T9.3.11): every iteration reshapes every character. Font data and
//! harfrust's shaping plans are warm, as they are after the first story.
//! All with the pinned test fonts, so the numbers do not depend on the host.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_geom::Mp;
use xarast_text::{
    FontDb, FontQuery, GenericName, Justification, ParagraphStyle, ScriptTag, Shaper, StoryInput,
    StoryMode, StyleRange,
};

fn db() -> Arc<FontDb> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
    let db = FontDb::new_isolated();
    for f in [
        "NotoSans-Regular.subset.ttf",
        "NotoSans-Bold.subset.ttf",
        "NotoSansHebrew-Regular.subset.ttf",
        "XarastTestVariable.ttf",
    ] {
        db.register_embedded(None, std::fs::read(dir.join(f)).expect("fixture"))
            .expect("parses");
    }
    db.set_generic_families(GenericName::SansSerif, &["Noto Sans"]);
    db.set_fallback_preference(ScriptTag::HEBREW, &["Noto Sans Hebrew"]);
    Arc::new(db)
}

const PROSE: &str = "The quick brown fox jumps over the lazy dog while five wizards box \
    and quietly judge the vexing jumble of text that wraps across the column. ";

fn paragraph(chars: usize) -> String {
    let mut s = String::new();
    while s.len() < chars {
        s.push_str(PROSE);
    }
    s.truncate(chars);
    s
}

fn story_10k() -> String {
    let mut s = String::new();
    for i in 0..20 {
        if i > 0 {
            s.push('\n');
        }
        let mut p = paragraph(470);
        if i % 4 == 1 {
            p.push_str(" שלום עולם ");
        }
        s.push_str(&p);
    }
    while s.chars().count() < 10_000 {
        s.push('x');
    }
    s
}

fn bench(c: &mut Criterion) {
    let shaper = Shaper::new(db());
    let para = ParagraphStyle {
        justification: Justification::Full,
        ..ParagraphStyle::default()
    };
    let col = StoryMode::column(Mp::new(400_000));

    let p1k = paragraph(1_000);
    let r1k = [StyleRange::new(
        0..p1k.len(),
        FontQuery::new("Noto Sans"),
        Mp::new(10_000),
    )];
    c.bench_function("paragraph_1k", |b| {
        b.iter(|| {
            black_box(shaper.layout(&StoryInput {
                text: black_box(&p1k),
                runs: &r1k,
                paragraphs: std::slice::from_ref(&para),
                kerns: &[],
                mode: col,
            }))
        })
    });

    let s10k = story_10k();
    // Alternate regular and bold every 500 bytes, as a styled story would.
    let runs: Vec<StyleRange> = (0..s10k.len())
        .step_by(500)
        .enumerate()
        .map(|(i, start)| {
            let end = (start + 500).min(s10k.len());
            let w = if i % 2 == 0 { 400 } else { 700 };
            StyleRange::new(
                start..end,
                FontQuery::new("Noto Sans").with_weight(w),
                Mp::new(10_000),
            )
        })
        .collect();
    let l = shaper.layout(&StoryInput {
        text: &s10k,
        runs: &runs,
        paragraphs: std::slice::from_ref(&para),
        kerns: &[],
        mode: col,
    });
    eprintln!(
        "story_10k: {} chars, {} lines, {} glyphs",
        s10k.chars().count(),
        l.lines.len(),
        l.glyphs().count()
    );
    c.bench_function("story_10k", |b| {
        b.iter(|| {
            black_box(shaper.layout(&StoryInput {
                text: black_box(&s10k),
                runs: &runs,
                paragraphs: std::slice::from_ref(&para),
                kerns: &[],
                mode: col,
            }))
        })
    });

    let fonts = shaper.fonts().clone();
    let face = fonts
        .query(&FontQuery::new("Noto Sans"))
        .expect("face")
        .face;
    let _ = fonts.glyph_outline(face, 34, &[]);
    c.bench_function("outline_cached", |b| {
        b.iter(|| black_box(fonts.glyph_outline(black_box(face), black_box(34), &[])))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
