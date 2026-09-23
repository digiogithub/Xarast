//! Foreign baggage (`research/06 §8`, XARA-T-0089): it rides on the node
//! through every edit the §8.7 battery names, undo and redo restore it
//! exactly, and it goes when the node is destroyed.

use std::sync::Arc;

use xarast_geom::{Matrix, Vector};

use crate::foreign::{ForeignAttr, ForeignBaggage, ForeignChild, ForeignChildKind, ForeignMarks};
use crate::history::{Command, CommandBus, EditError, Tx};
use crate::kind::NodeKind;
use crate::tests::{black_fill, fixture, layer};
use crate::tree::{Attach, NodeId};

fn baggage(tag: &str) -> ForeignBaggage {
    ForeignBaggage {
        attrs: vec![ForeignAttr {
            ns: Arc::from("urn:test:future"),
            prefix: Some(Arc::from("fut")),
            local: Arc::from("state"),
            value: Arc::from(tag),
        }],
        children: vec![
            ForeignChild {
                position: 0,
                kind: ForeignChildKind::Element,
                raw: Arc::from(
                    format!("<fut:thing xmlns:fut=\"urn:test:future\" v='{tag}'/>").as_str(),
                ),
            },
            ForeignChild {
                position: 1,
                kind: ForeignChildKind::Comment,
                raw: Arc::from("<!-- a note -->"),
            },
        ],
        marks: ForeignMarks::empty(),
    }
}

struct Run<F>(F);

impl<F> std::fmt::Debug for Run<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Run")
    }
}

impl<F: Fn(&mut Tx<'_>) -> Result<(), EditError>> Command for Run<F> {
    fn label(&self) -> &'static str {
        "run"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        (self.0)(tx)
    }
}

fn run<F: Fn(&mut Tx<'_>) -> Result<(), EditError>>(
    bus: &mut CommandBus,
    doc: &mut crate::Document,
    f: F,
) {
    bus.dispatch(doc, &Run(f)).unwrap();
}

#[test]
fn baggage_survives_the_preservation_battery() {
    let f = fixture();
    let (layer_id, path, shape) = (f.layer, f.path, f.shape_a);
    let mut doc = f.doc;
    let mut bus = CommandBus::new();

    run(&mut bus, &mut doc, |tx| {
        tx.set_foreign(path, Some(baggage("p")))?;
        tx.set_foreign(shape, Some(baggage("s")))
    });
    let with_baggage = doc.canonical_digest();

    // Move.
    run(&mut bus, &mut doc, |tx| {
        tx.transform(path, Matrix::translate(Vector::raw(1_000, 2_000)))
    });
    // Recolour: a new fill attribute as the path's child.
    run(&mut bus, &mut doc, |tx| {
        let a = tx.create(NodeKind::Attr(Box::new(crate::attr::AttrNode::new(
            black_fill(),
        ))))?;
        tx.attach(a, path, Attach::FirstChild)
    });
    // Group: wrap the path in a new group.
    run(&mut bus, &mut doc, |tx| {
        let g = tx.create(NodeKind::Group(Box::default()))?;
        tx.attach(g, layer_id, Attach::LastChild)?;
        tx.move_node(path, g, Attach::LastChild)
    });
    // Change layer.
    run(&mut bus, &mut doc, |tx| {
        let l = tx.create(layer("other"))?;
        let spread = tx.doc().tree.links(layer_id).parent.unwrap();
        tx.attach(l, spread, Attach::LastChild)?;
        tx.move_node(shape, l, Attach::LastChild)
    });
    // Delete, which the history retains.
    run(&mut bus, &mut doc, |tx| tx.delete(shape));

    assert_eq!(doc.tree.foreign(path), Some(&baggage("p")));
    assert_eq!(doc.tree.foreign(shape), Some(&baggage("s")));
    doc.validate().assert_clean();
    let end = doc.canonical_digest();

    while bus.history_mut().undo(&mut doc).is_some() {}
    assert!(doc.tree.foreign(path).is_none());
    assert_eq!(doc.tree.foreign_len(), 0);
    while bus.history_mut().redo(&mut doc).is_some() {}
    assert_eq!(end, doc.canonical_digest());
    assert_eq!(doc.tree.foreign(shape), Some(&baggage("s")));

    // Undo back to just after the baggage went on: exactly that document.
    for _ in 0..5 {
        bus.history_mut().undo(&mut doc).unwrap();
    }
    assert_eq!(with_baggage, doc.canonical_digest());
}

#[test]
fn baggage_is_part_of_the_digest_and_its_absence_changes_nothing() {
    let f = fixture();
    let plain = f.doc.canonical_digest();
    let mut doc = f.doc;
    let mut bus = CommandBus::new();
    run(&mut bus, &mut doc, |tx| {
        tx.set_foreign(f.path, Some(baggage("x")))
    });
    let a = doc.canonical_digest();
    assert_ne!(plain, a);
    run(&mut bus, &mut doc, |tx| {
        tx.set_foreign(f.path, Some(baggage("y")))
    });
    assert_ne!(a, doc.canonical_digest());
    // Empty baggage is no baggage.
    run(&mut bus, &mut doc, |tx| {
        tx.set_foreign(f.path, Some(ForeignBaggage::default()))
    });
    assert_eq!(plain, doc.canonical_digest());
    assert_eq!(doc.tree.foreign_len(), 0);
}

#[test]
fn marking_adds_marks_once_and_only_where_there_is_baggage() {
    let f = fixture();
    let mut doc = f.doc;
    let mut bus = CommandBus::new();
    run(&mut bus, &mut doc, |tx| {
        tx.set_foreign(f.path, Some(baggage("m")))
    });
    let before = bus.history_mut().len();
    run(&mut bus, &mut doc, |tx| {
        tx.mark_foreign(f.path, ForeignMarks::DIRTY)?;
        tx.mark_foreign(f.shape_a, ForeignMarks::STALE)
    });
    assert_eq!(
        doc.tree.foreign(f.path).map(|b| b.marks),
        Some(ForeignMarks::DIRTY)
    );
    assert!(doc.tree.foreign(f.shape_a).is_none());
    assert_eq!(bus.history_mut().len(), before + 1);
    bus.history_mut().undo(&mut doc).unwrap();
    assert_eq!(
        doc.tree.foreign(f.path).map(|b| b.marks),
        Some(ForeignMarks::empty())
    );
}

#[test]
fn destroying_a_node_drops_its_baggage() {
    let f = fixture();
    let mut doc = f.doc;
    doc.tree.set_foreign(f.group, Some(Arc::new(baggage("g"))));
    doc.tree
        .set_foreign(f.shape_b, Some(Arc::new(baggage("b"))));
    assert_eq!(doc.tree.foreign_len(), 2);
    doc.tree.destroy_subtree(f.group);
    assert_eq!(doc.tree.foreign_len(), 0);
    assert!(doc.tree.foreign(f.shape_b).is_none());
}

#[test]
fn a_snapshot_carries_baggage_onto_the_new_ids() {
    let f = fixture();
    let mut doc = f.doc;
    doc.tree.set_foreign(f.path, Some(Arc::new(baggage("s"))));
    let root = doc.tree.root();
    doc.tree.set_foreign(root, Some(Arc::new(baggage("root"))));
    let digest = doc.canonical_digest();
    let snap = doc.snapshot();
    doc.restore(&snap);
    assert_eq!(digest, doc.canonical_digest());
    assert_eq!(doc.tree.foreign_len(), 2);
    let root = doc.tree.root();
    assert_eq!(doc.tree.foreign(root), Some(&baggage("root")));
}

#[test]
fn the_builder_attaches_baggage() {
    let mut b = crate::builder::skeleton(crate::BuildLimits::default()).unwrap();
    let id = b.node(NodeKind::Group(Box::default())).unwrap();
    b.foreign(id, baggage("built"));
    let (doc, _) = b.finish().unwrap();
    let with: Vec<NodeId> = doc.tree.foreign_iter().map(|(n, _)| n).collect();
    assert_eq!(with.len(), 1);
    assert_eq!(doc.tree.foreign(with[0]), Some(&baggage("built")));
}

#[test]
fn node_data_did_not_grow() {
    assert!(size_of::<crate::NodeData>() <= 64);
}
