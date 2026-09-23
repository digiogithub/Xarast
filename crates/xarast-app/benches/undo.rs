//! Undo and redo of one edit, the phase-7 budget of ≤ 1 ms.
//!
//! `cargo bench -p xarast-app -- undo`
//!
//! Over the synthetic document at 250 000 nodes (about 100 000 objects):
//!
//! * `undo/move_one` — `Session::apply(Intent::Undo)` then
//!   `Session::apply(Intent::Redo)` of a move of one object, divided by two:
//!   the bus, the session's bookkeeping (selection prune, scroll bounds,
//!   dirty tracking) and nothing else. The scene rebuild the frame then
//!   owes is the render budget's, not this one's.
//! * `undo/bus_only` — the same through the bare `CommandBus`, for
//!   reference.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_app::{DocumentId, EditCommand, Intent, Session};
use xarast_doc::{CommandBus, SynthSpec, synthetic_document};
use xarast_geom::Vector;

fn doc() -> xarast_doc::Document {
    synthetic_document(SynthSpec {
        nodes: 250_000,
        ..SynthSpec::default()
    })
}

/// The smallest selectable object: "one edit" is a move of one shape, not
/// of a layer-sized group of thousands.
fn first_object(doc: &xarast_doc::Document) -> xarast_doc::NodeId {
    xarast_app::edit::selectable_objects(doc)
        .min_by_key(|n| doc.tree.preorder(*n).count())
        .expect("an object")
}

fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("undo");

    let mut s = Session::adopt(DocumentId(1), doc(), None);
    let n = first_object(&s.doc);
    s.apply_edit(EditCommand::translate(vec![n], Vector::raw(1_000, 0)))
        .expect("move");
    g.bench_function("move_one", |b| {
        b.iter(|| {
            black_box(s.apply(Intent::Undo).expect("undo"));
            black_box(s.apply(Intent::Redo).expect("redo"));
        });
    });

    let mut d = doc();
    let n = first_object(&d);
    let mut bus = CommandBus::new();
    bus.dispatch(
        &mut d,
        &EditCommand::translate(vec![n], Vector::raw(1_000, 0)),
    )
    .expect("move");
    g.bench_function("bus_only", |b| {
        b.iter(|| {
            black_box(bus.undo(&mut d));
            black_box(bus.redo(&mut d));
        });
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
