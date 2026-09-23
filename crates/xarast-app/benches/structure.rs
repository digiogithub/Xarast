//! Structure operations over 5 000 objects: the phase-7 budgets "group of
//! 5 000 objects ≤ 50 ms" and "align/distribute of 5 000 objects ≤ 50 ms".
//!
//! `cargo bench -p xarast-app --bench structure`. Each iteration applies
//! the operation through the session and undoes it, so the figure is the
//! pair.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_app::structure::{AlignSpec, AlignTarget, AxisAlign};
use xarast_app::{DocumentId, Intent, SelectMode, Session};
use xarast_doc::{
    Attach, AttrNode, AttrValue, Command, EditError, NodeId, NodeKind, ShapeKind, ShapeNode, Tx,
};
use xarast_geom::{Point, Vector};

#[derive(Debug)]
struct Squares(usize);

impl Command for Squares {
    fn label(&self) -> &'static str {
        "Fixture"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let layer = tx.doc().active_layer(tx.doc().active_spread()).unwrap();
        for i in 0..self.0 {
            let (x, y) = ((i % 100) as i32 * 6_000, (i / 100) as i32 * 6_000);
            let n = tx.create(NodeKind::Shape(Box::new(ShapeNode {
                shape: ShapeKind::Rect,
                origin: Point::raw(x, y),
                major: Vector::raw(5_000, 0),
                minor: Vector::raw(0, 5_000),
            })))?;
            tx.attach(n, layer, Attach::LastChild)?;
            // Every tenth square carries a loose fill before it, so the
            // group has attributes to localise.
            if i % 10 == 0 {
                let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(AttrValue::Fill(
                    xarast_doc::fill::Paint::Flat {
                        value: xarast_color::Colour::Direct(xarast_color::ColourValue::rgbt(
                            (i % 7) as f32 / 7.0,
                            0.3,
                            0.6,
                            0.0,
                        )),
                    },
                )))))?;
                tx.attach(a, n, Attach::Prev)?;
            }
        }
        Ok(())
    }
}

fn session(n: usize) -> (Session, Vec<NodeId>) {
    let mut s = Session::new_empty(DocumentId(1));
    s.dispatch(&Squares(n)).expect("fixture");
    s.bus.history_mut().clear(&mut s.doc);
    let objs: Vec<NodeId> = xarast_app::edit::selectable_objects(&s.doc).collect();
    s.apply(Intent::Select {
        nodes: objs.clone(),
        mode: SelectMode::Replace,
    })
    .expect("select");
    (s, objs)
}

fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("structure");
    g.sample_size(10);
    let (mut s, objs) = session(5_000);
    g.bench_function("group 5000 + undo", |b| {
        b.iter(|| {
            black_box(s.apply(Intent::Group).expect("group"));
            black_box(s.undo());
            s.apply(Intent::Select {
                nodes: objs.clone(),
                mode: SelectMode::Replace,
            })
            .expect("select");
        });
    });
    let (mut s, _) = session(5_000);
    let spec = AlignSpec {
        x: AxisAlign::Min,
        y: AxisAlign::DistributeCentre,
        to: AlignTarget::Selection,
    };
    g.bench_function("align + distribute 5000 + undo", |b| {
        b.iter(|| {
            black_box(s.apply(Intent::Align(spec)).expect("align"));
            black_box(s.undo());
        });
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
