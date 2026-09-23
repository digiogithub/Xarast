//! Picking over the synthetic document at 250 000 nodes (about 100 000
//! objects): building the pick index (paid once after each committed
//! change) and a precise pick with the index warm (every click).
//!
//! `cargo bench -p xarast-app --bench pick`. Budgets (`phase-07`): a hit
//! test ≤ 2 ms worst case.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_app::tool::{PICK_TOLERANCE_PX, PickMode, Picker};
use xarast_doc::{SynthSpec, synthetic_document};

fn bench(c: &mut Criterion) {
    let doc = synthetic_document(SynthSpec {
        nodes: 250_000,
        ..SynthSpec::default()
    });
    let bounds = xarast_app::viewport::drawing_or_page_rect(&doc);
    let centre = bounds.centre();
    let mut g = c.benchmark_group("pick");
    g.sample_size(10);
    g.bench_function("build index and pick (first click after an edit)", |b| {
        b.iter(|| {
            let p = Picker::new();
            black_box(p.pick(&doc, centre, PICK_TOLERANCE_PX, 750.0, PickMode::TopGroup))
        });
    });
    let p = Picker::new();
    let _ = p.pick(&doc, centre, PICK_TOLERANCE_PX, 750.0, PickMode::TopGroup);
    g.sample_size(100);
    g.bench_function("pick, index warm", |b| {
        b.iter(|| black_box(p.pick(&doc, centre, PICK_TOLERANCE_PX, 750.0, PickMode::TopGroup)));
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
