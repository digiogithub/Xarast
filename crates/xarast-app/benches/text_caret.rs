//! Caret movement over a 10 000-character story: the phase-9 budget
//! "caret movement (any motion) ≤ 1 ms".
//!
//! `cargo bench -p xarast-app --bench text_caret`. Pinned fonts; a column
//! of Latin and Hebrew paragraphs, so lines wrap and runs change direction.

use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_app::fonts::FontService;
use xarast_app::text_edit::{Caret, CaretMap, CaretMotion};
use xarast_geom::Mp;
use xarast_text::{FontQuery, ParagraphStyle, StoryInput, StoryMode, StyleRange};

fn story() -> CaretMap {
    let fonts = FontService::from_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"),
    );
    let mut text = String::new();
    let mut n = 0;
    while text.len() < 10_000 {
        text.push_str("The quick brown fox jumps over שלום עולם the lazy dog 123. ");
        n += 1;
        if n % 8 == 0 {
            text.push('\n');
        }
    }
    let runs = [StyleRange::new(
        0..text.len(),
        FontQuery::new("Noto Sans"),
        Mp::new(10_000),
    )];
    let layout = fonts.ready().layout(&StoryInput {
        text: &text,
        runs: &runs,
        paragraphs: &[ParagraphStyle::default()],
        kerns: &[],
        mode: StoryMode::column(Mp::new(300_000)),
    });
    CaretMap::new(text, layout)
}

fn bench(c: &mut Criterion) {
    let m = story();
    let mid = m.snap(Caret::at(m.len() / 2));
    let mut g = c.benchmark_group("text_caret");
    for (name, motion, fwd) in [
        (
            "visual right",
            CaretMotion::Character { visual: true },
            true,
        ),
        (
            "logical right",
            CaretMotion::Character { visual: false },
            true,
        ),
        ("word right", CaretMotion::Word, true),
        ("line down", CaretMotion::Line, true),
        ("line end", CaretMotion::LineEnd, true),
    ] {
        g.bench_function(name, |b| {
            b.iter(|| black_box(m.move_caret(black_box(mid), motion, fwd, None)));
        });
    }
    g.bench_function("hit", |b| {
        b.iter(|| black_box(m.hit(black_box(Mp::new(150_000)), black_box(Mp::new(-500_000)))));
    });
    g.bench_function("selection of everything", |b| {
        b.iter(|| black_box(m.selection_rects(0, m.len())));
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
