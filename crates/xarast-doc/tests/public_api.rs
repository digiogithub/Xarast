//! What a downstream crate can actually do.
//!
//! This file is a separate crate from the library, so it sees exactly the
//! surface an importer or a tool sees — which is the point: the arena's
//! mutating half is invisible from here.

use std::sync::Arc;

use xarast_doc::{
    Attach, AttrSlot, AttrStack, AttrValue, BuildLimits, Command, CommandBus, DiagCode, Document,
    DocumentBuilder, DumpOptions, EditError, NodeId, NodeKind, PathNode, Tx, WalkEvent,
};
use xarast_geom::{Matrix, Mp, Point, Rect, Vector};

fn square(at: Point, side: i32) -> xarast_geom::Path {
    let mut b = xarast_geom::Path::builder();
    b.rect(Rect::new(
        at,
        Point::new(at.x + Mp::new(side), at.y + Mp::new(side)),
    ));
    b.build()
}

/// An importer, in miniature: it only ever talks to the builder.
fn import() -> (Document, Vec<xarast_doc::Diagnostic>) {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::small()).expect("skeleton");
    b.attribute(AttrValue::LineWidth(Mp::new(2_000)))
        .expect("attribute");
    b.node(NodeKind::Group(Box::default())).expect("group");
    b.push_scope().expect("scope");
    b.attribute(AttrValue::WindingRule(xarast_geom::FillRule::EvenOdd))
        .expect("attribute");
    for i in 0..4 {
        b.node(NodeKind::Path(Box::new(PathNode::new(square(
            Point::raw(i * 1_000, 0),
            900,
        )))))
        .expect("path");
    }
    b.pop_scope();
    b.finish().expect("finish")
}

#[test]
fn an_importer_can_only_reach_the_builder_and_it_produces_a_valid_document() {
    let (doc, diags) = import();
    doc.validate().assert_clean();
    assert!(diags.iter().all(|d| d.code != DiagCode::IllegalNesting));
    assert!(doc.tree.node_count() > 8);
}

#[test]
fn a_tool_can_only_reach_the_command_bus() {
    #[derive(Debug)]
    struct MoveIt(NodeId);
    impl Command for MoveIt {
        fn label(&self) -> &'static str {
            "move"
        }
        fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
            tx.transform(
                self.0,
                Matrix::translate(Vector::new(Mp::new(1_000), Mp::new(500))),
            )
        }
    }

    let (mut doc, _) = import();
    let group = doc
        .tree
        .preorder(doc.tree.root())
        .find(|n| matches!(doc.tree.kind(*n), Some(NodeKind::Group(_))))
        .expect("the group");
    let before = doc.canonical_digest();
    let mut bus = CommandBus::new();
    assert_eq!(bus.dispatch(&mut doc, &MoveIt(group)).unwrap(), "move");
    assert_ne!(before, doc.canonical_digest());
    bus.history_mut().undo(&mut doc).expect("undo");
    assert_eq!(before, doc.canonical_digest());
    doc.validate().assert_clean();
}

#[test]
fn a_scene_builder_can_walk_the_document_with_an_attribute_stack() {
    let (doc, _) = import();
    let mut stack = AttrStack::with_defaults(&doc.defaults);
    let mut painted = 0usize;
    let mut widths = Vec::new();
    for ev in doc.tree.walk_render(doc.tree.root()) {
        match ev {
            WalkEvent::EnterScope { .. } => stack.push_scope(),
            WalkEvent::Visit { node } => match doc.tree.kind(node) {
                Some(NodeKind::Attr(a)) => stack.push(Arc::new(a.value.clone())),
                Some(k) if k.is_ink() && doc.tree.links(node).first_child.is_none() => {
                    painted += 1;
                    widths.push(stack.get(AttrSlot::LineWidth).clone());
                }
                _ => {}
            },
            WalkEvent::LeaveScope { .. } => stack.pop_scope(),
        }
    }
    assert_eq!(painted, 4);
    assert!(
        widths
            .iter()
            .all(|w| *w == AttrValue::LineWidth(Mp::new(2_000))),
        "the layer's line width reaches into the group"
    );
}

#[test]
fn the_canonical_document_dumps_the_shape_the_research_describes() {
    let doc = Document::new_empty();
    let dump = doc.dump(DumpOptions {
        show_tags: true,
        ..DumpOptions::default()
    });
    assert!(dump.starts_with("Document #0\n  Chapter #1\n    Spread"));
    assert!(dump.contains("Layer \"Layer 1\" active"));
}

#[test]
fn an_empty_builder_is_usable_from_outside() {
    let mut b = DocumentBuilder::new(BuildLimits::default());
    b.node(NodeKind::Chapter).expect("chapter");
    let (doc, _) = b.finish().expect("finish");
    doc.validate().assert_clean();
    // The public surface exposes reading, not writing.
    assert_eq!(doc.tree.children(doc.tree.root()).count(), 1);
    let _ = Attach::LastChild;
}
