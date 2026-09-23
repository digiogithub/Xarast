//! Picking over the synthetic document at 250 000 nodes (about 100 000
//! objects): building the pick index (paid once after each committed
//! change) and a precise pick with the index warm (every click).
//!
//! `cargo bench -p xarast-app --bench pick`. Budgets (`phase-07`): a hit
//! test ≤ 2 ms worst case.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_app::tool::{PICK_TOLERANCE_PX, PickMode, Picker};
use xarast_app::{DocumentId, EditCommand, Intent, Session};
use xarast_doc::{SynthSpec, synthetic_document};
use xarast_geom::Vector;

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

    // XARA-T-0168: the first pick after a one-object edit, through the
    // session's incrementally kept index. Undo then pick, redo then pick:
    // each pick applies one step's journal.
    let mut s = Session::adopt(DocumentId(1), doc, None);
    let n = xarast_app::edit::selectable_objects(&s.doc)
        .min_by_key(|n| s.doc.tree.preorder(*n).count())
        .expect("an object");
    s.apply_edit(EditCommand::translate(vec![n], Vector::raw(1_000, 0)))
        .expect("move");
    let rebuilds = s.picker().rebuilds();
    let _ = s
        .picker()
        .pick(&s.doc, centre, PICK_TOLERANCE_PX, 750.0, PickMode::TopGroup);
    g.bench_function("undo + first pick after it (incremental)", |b| {
        b.iter(|| {
            black_box(s.apply(Intent::Undo).expect("undo"));
            black_box(s.picker().pick(
                &s.doc,
                centre,
                PICK_TOLERANCE_PX,
                750.0,
                PickMode::TopGroup,
            ));
            black_box(s.apply(Intent::Redo).expect("redo"));
            black_box(s.picker().pick(
                &s.doc,
                centre,
                PICK_TOLERANCE_PX,
                750.0,
                PickMode::TopGroup,
            ));
        });
    });
    assert!(
        s.picker().rebuilds() <= rebuilds + 1,
        "the edits were applied incrementally"
    );

    // XARA-US-0042: resolving a colour drag's drop target, once per pointer
    // move (`phase-08` budget: 1 ms at 100k objects). Two points a pixel
    // apart, so every move resolves again.
    use xarast_app::colour_bar::{ColourBarOp, ColourSource, DragPoint};
    let (cx, cy) = centre.to_f64();
    s.viewport
        .set_centre(xarast_app::geometry::DocPointF::new(cx, cy));
    let a = s.viewport.doc_to_device(centre);
    let b2 = xarast_app::DevicePoint::new(a.x + 1.0, a.y);
    s.apply(Intent::ColourBar(ColourBarOp::DragBegin(
        ColourSource::NoColour,
    )))
    .expect("drag");
    g.bench_function("colour drag: resolve the drop target per move", |b| {
        b.iter(|| {
            for at in [a, b2] {
                black_box(
                    s.apply(Intent::ColourBar(ColourBarOp::DragTo(DragPoint::Canvas {
                        at,
                        shift: false,
                    })))
                    .expect("move"),
                );
            }
        });
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
