//! The Phase 2 performance budgets.
//!
//! Measured against the synthetic 100 000-node document of
//! `xarast_doc::synth`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use xarast_doc::{
    Attach, AttrSlot, AttrStack, AttrValue, Command, CommandBus, Document, EditError, NodeId,
    SynthSpec, Tx, WalkEvent, synthetic_document,
};
use xarast_geom::{Matrix, Mp, Vector};

fn doc() -> Document {
    synthetic_document(SynthSpec::default())
}

#[derive(Debug)]
struct Nudge(NodeId);

impl Command for Nudge {
    fn label(&self) -> &'static str {
        "nudge"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.transform(
            self.0,
            Matrix::translate(Vector::new(Mp::new(10), Mp::ZERO)),
        )
    }
}

fn benches(c: &mut Criterion) {
    let d = doc();
    let root = d.tree.root();
    let ids: Vec<NodeId> = d.tree.preorder(root).collect();
    println!("synthetic document: {} nodes", ids.len());

    c.bench_function("walk_render/100k", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for ev in d.tree.walk_render(root) {
                if let WalkEvent::Visit { .. } = ev {
                    n += 1;
                }
            }
            black_box(n)
        });
    });

    c.bench_function("preorder/100k", |b| {
        b.iter(|| black_box(d.tree.preorder(root).count()));
    });

    c.bench_function("get_by_id/random", |b| {
        let mut i = 0usize;
        b.iter(|| {
            i = i.wrapping_mul(2_654_435_761).wrapping_add(1) % ids.len();
            black_box(d.tree.get(ids[i]).is_some())
        });
    });

    c.bench_function("attr_stack/push+pop_scope", |b| {
        let mut s = AttrStack::with_defaults(&d.defaults);
        let v = std::sync::Arc::new(AttrValue::LineWidth(Mp::new(1_000)));
        b.iter(|| {
            s.push_scope();
            s.push(v.clone());
            s.pop_scope();
        });
    });

    {
        let mut d2 = doc();
        let target = *ids.last().expect("a node");
        let defaults = d2.defaults.clone();
        c.bench_function("attr_resolver/warm", |b| {
            let _ = d2.attrs.resolve(&d2.tree, target, &defaults);
            b.iter(|| {
                black_box(
                    d2.attrs
                        .resolve(&d2.tree, target, &defaults)
                        .get(AttrSlot::LineWidth)
                        .clone(),
                )
            });
        });
        c.bench_function("attr_resolver/cold", |b| {
            b.iter(|| {
                d2.attrs.invalidate_all();
                black_box(
                    d2.attrs
                        .resolve(&d2.tree, target, &defaults)
                        .get(AttrSlot::LineWidth)
                        .clone(),
                )
            });
        });
    }

    c.bench_function("builder/100k_nodes", |b| {
        b.iter(|| black_box(synthetic_document(SynthSpec::default()).tree.node_count()));
    });

    c.bench_function("canonical_digest/100k", |b| {
        b.iter(|| black_box(d.canonical_digest()));
    });

    c.bench_function("snapshot/100k", |b| {
        b.iter(|| black_box(d.snapshot().len()));
    });

    {
        let mut d3 = doc();
        let mut bus = CommandBus::new();
        let target = d3
            .tree
            .preorder(d3.tree.root())
            .nth(500)
            .expect("a node to nudge");
        c.bench_function("undo/single_node_edit", |b| {
            b.iter(|| {
                bus.dispatch(&mut d3, &Nudge(target)).expect("nudge");
                bus.history_mut().undo(&mut d3).expect("undo");
            });
        });
        c.bench_function("redo/single_node_edit", |b| {
            bus.dispatch(&mut d3, &Nudge(target)).expect("nudge");
            bus.history_mut().undo(&mut d3).expect("undo");
            b.iter(|| {
                bus.history_mut().redo(&mut d3).expect("redo");
                bus.history_mut().undo(&mut d3).expect("undo");
            });
        });
    }

    {
        let mut d4 = doc();
        c.bench_function("compute_bounds/100k_cold", |b| {
            b.iter(|| {
                d4.update_bounds();
                black_box(d4.tree.bounds(d4.tree.root()).is_valid())
            });
        });
    }

    // Keep `Attach` used, so that the bench file also documents the API it
    // exercises.
    let _ = Attach::LastChild;
}

criterion_group!(doc_benches, benches);
criterion_main!(doc_benches);
