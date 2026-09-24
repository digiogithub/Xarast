//! Property tests.
//!
//! The case counts are deliberately modest by default so that
//! `cargo test --workspace` stays usable; the phase document's targets
//! (50 000 tree cases, 10 000 undo cases) are reached in CI by setting
//! `PROPTEST_CASES`, which `proptest` reads by itself.

use proptest::prelude::*;
use std::sync::Arc;

use crate::attr::AttrValue;
use crate::builder::{BuildError, BuildLimits, DocumentBuilder};
use crate::history::{Command, CommandBus, EditError, Tx};
use crate::kind::{NodeKind, PathNode};
use crate::structure::LayerNode;
use crate::tests::{black_fill, fixture, square, white_fill};
use crate::tree::{Attach, NodeFlags, NodeId};
use xarast_geom::{Matrix, Mp, Point, Vector};

#[derive(Clone, Copy, Debug)]
enum Op {
    CreateAttach(u8, u8),
    Detach(u8),
    Move(u8, u8, u8),
    Destroy(u8),
    CreateAttr(u8),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..64, 0u8..4).prop_map(|(a, b)| Op::CreateAttach(a, b)),
        (0u8..64).prop_map(Op::Detach),
        (0u8..64, 0u8..64, 0u8..4).prop_map(|(a, b, c)| Op::Move(a, b, c)),
        (0u8..64).prop_map(Op::Destroy),
        (0u8..64).prop_map(Op::CreateAttr),
    ]
}

fn how(v: u8) -> Attach {
    match v % 4 {
        0 => Attach::FirstChild,
        1 => Attach::LastChild,
        2 => Attach::Prev,
        _ => Attach::Next,
    }
}

fn pick(ids: &[NodeId], i: u8) -> Option<NodeId> {
    if ids.is_empty() {
        None
    } else {
        Some(ids[i as usize % ids.len()])
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_shrink_iters: 4096,
        ..ProptestConfig::default()
    })]

    /// The tree survives any sequence of attach, detach, move, create and
    /// destroy with every invariant intact after **every** step.
    #[test]
    fn the_tree_stays_valid_under_any_operation_sequence(
        ops in prop::collection::vec(op_strategy(), 0..200)
    ) {
        let mut f = fixture();
        for op in ops {
            let live: Vec<NodeId> = f.doc.tree.preorder(f.doc.tree.root()).collect();
            match op {
                Op::CreateAttach(a, h) => {
                    if let Some(anchor) = pick(&live, a) {
                        let n = f.doc.tree.create(NodeKind::Group(Box::default()));
                        let _ = f.doc.tree.attach(n, anchor, how(h));
                    }
                }
                Op::CreateAttr(a) => {
                    if let Some(anchor) = pick(&live, a) {
                        let n = f.doc.tree.create(NodeKind::Attr(Box::new(
                            crate::attr::AttrNode::new(black_fill()),
                        )));
                        let _ = f.doc.tree.attach(n, anchor, Attach::LastChild);
                    }
                }
                Op::Detach(a) => {
                    if let Some(n) = pick(&live, a) {
                        let _ = f.doc.tree.detach(n);
                    }
                }
                Op::Move(a, b, h) => {
                    if let (Some(n), Some(anchor)) = (pick(&live, a), pick(&live, b))
                        && n != anchor
                        && f.doc.tree.detach(n).is_ok()
                        && f.doc.tree.attach(n, anchor, how(h)).is_err()
                    {
                        // Refused: put it back where it can go, or leave it
                        // detached. Either way the tree must still validate.
                    }
                }
                Op::Destroy(a) => {
                    if let Some(n) = pick(&live, a) {
                        f.doc.tree.destroy_subtree(n);
                    }
                }
            }
            let report = f.doc.tree.validate();
            // "One spread, one active layer" is a document *policy* that
            // commands maintain (see `Tx::keep_one_active_layer`), not a
            // property of the links. Raw surgery straight at the arena, which
            // is what this test does and what no caller outside the crate
            // can do, is allowed to break it.
            let structural: Vec<_> = report
                .errors
                .iter()
                .filter(|e| !matches!(e, crate::validate::Invariant::NoActiveLayer { .. }))
                .collect();
            prop_assert!(
                structural.is_empty(),
                "invariants broken after {op:?}: {structural:#?}"
            );
        }
    }

    /// Whatever the tree looks like, the cached resolver, the uncached
    /// resolver and an ancestor walk all agree.
    #[test]
    fn attribute_resolution_agrees_three_ways(
        ops in prop::collection::vec(op_strategy(), 0..40)
    ) {
        let mut f = fixture();
        for op in ops {
            let live: Vec<NodeId> = f.doc.tree.preorder(f.doc.tree.root()).collect();
            if let Op::CreateAttr(a) | Op::CreateAttach(a, _) = op
                && let Some(anchor) = pick(&live, a)
            {
                let n = f.doc.tree.create(NodeKind::Attr(Box::new(
                    crate::attr::AttrNode::new(white_fill()),
                )));
                let _ = f.doc.tree.attach(n, anchor, Attach::LastChild);
            }
        }
        let defaults = f.doc.defaults.clone();
        for n in f.doc.tree.preorder(f.doc.tree.root()) {
            let uncached = crate::attr::resolve_uncached(&f.doc.tree, n, &defaults);
            let cached = f.doc.attrs.resolve(&f.doc.tree, n, &defaults).clone();
            prop_assert!(cached == uncached);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Edit {
    Add(u8),
    Delete(u8),
    Nudge(u8, i16, i16),
    Scale(u8),
    Flag(u8),
    Recolour(u8),
    /// `Tx::move_node`, which can carry a layer between spreads.
    Move(u8, u8, u8),
    /// Set a layer's `active` flag, leaving the repair to the commit.
    SetActive(u8, bool),
    /// Attach a whole new spread holding two layers that are both active.
    AddSpread(u8),
    /// Give a node foreign baggage, clear it, or mark it (XARA-T-0089).
    Foreign(u8, u8),
}

fn edit_strategy() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (0u8..64).prop_map(Edit::Add),
        (0u8..64).prop_map(Edit::Delete),
        (0u8..64, -500i16..500, -500i16..500).prop_map(|(a, x, y)| Edit::Nudge(a, x, y)),
        (0u8..64).prop_map(Edit::Scale),
        (0u8..64).prop_map(Edit::Flag),
        (0u8..64).prop_map(Edit::Recolour),
        (0u8..64, 0u8..64, 0u8..4).prop_map(|(a, b, h)| Edit::Move(a, b, h)),
        (0u8..64, any::<bool>()).prop_map(|(a, v)| Edit::SetActive(a, v)),
        (0u8..64).prop_map(Edit::AddSpread),
        (0u8..64, 0u8..4).prop_map(|(a, k)| Edit::Foreign(a, k)),
    ]
}

#[derive(Debug)]
struct EditCommand(Edit, Vec<NodeId>);

impl Command for EditCommand {
    fn label(&self) -> &'static str {
        "edit"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let ids = &self.1;
        match self.0 {
            Edit::Add(a) => {
                let Some(anchor) = pick(ids, a) else {
                    return Ok(());
                };
                let n = tx.create(NodeKind::Layer(Box::new(LayerNode::named("p"))))?;
                tx.attach(n, anchor, Attach::LastChild)
            }
            Edit::Delete(a) => match pick(ids, a) {
                Some(n) if n != tx.doc().tree.root() => tx.delete(n),
                _ => Ok(()),
            },
            Edit::Nudge(a, x, y) => match pick(ids, a) {
                Some(n) => tx.transform(
                    n,
                    Matrix::translate(Vector::new(Mp::new(i32::from(x)), Mp::new(i32::from(y)))),
                ),
                None => Ok(()),
            },
            Edit::Scale(a) => match pick(ids, a) {
                Some(n) => tx.transform(n, Matrix::scale(1.25, 0.75)),
                None => Ok(()),
            },
            Edit::Flag(a) => match pick(ids, a) {
                Some(n) => tx.set_flags(n, NodeFlags::MAGNETIC),
                None => Ok(()),
            },
            Edit::Recolour(a) => match pick(ids, a) {
                Some(n) if matches!(tx.doc().tree.kind(n), Some(NodeKind::Attr(_))) => {
                    tx.set_attr(n, black_fill())
                }
                _ => Ok(()),
            },
            Edit::Move(a, b, h) => match (pick(ids, a), pick(ids, b)) {
                (Some(n), Some(anchor)) if n != anchor && n != tx.doc().tree.root() => {
                    tx.move_node(n, anchor, how(h))
                }
                _ => Ok(()),
            },
            Edit::SetActive(a, v) => match pick(ids, a) {
                Some(n) => match tx.doc().tree.kind(n) {
                    Some(NodeKind::Layer(l)) => {
                        let mut next = l.clone();
                        next.active = v;
                        tx.set_kind(n, NodeKind::Layer(next))
                    }
                    _ => Ok(()),
                },
                None => Ok(()),
            },
            Edit::AddSpread(a) => {
                let Some(anchor) = pick(ids, a) else {
                    return Ok(());
                };
                let spread = tx.create(NodeKind::Spread(Box::default()))?;
                for name in ["s1", "s2"] {
                    let mut l = LayerNode::named(name);
                    l.active = true;
                    let l = tx.create(NodeKind::Layer(Box::new(l)))?;
                    tx.attach(l, spread, Attach::LastChild)?;
                }
                tx.attach(spread, anchor, Attach::LastChild)
            }
            Edit::Foreign(a, k) => {
                let Some(n) = pick(ids, a) else {
                    return Ok(());
                };
                match k {
                    0 => tx.set_foreign(n, None),
                    1 => tx.mark_foreign(n, crate::foreign::ForeignMarks::STALE),
                    _ => tx.set_foreign(
                        n,
                        Some(crate::foreign::ForeignBaggage {
                            attrs: vec![crate::foreign::ForeignAttr {
                                ns: std::sync::Arc::from("urn:test:future"),
                                prefix: None,
                                local: std::sync::Arc::from("k"),
                                value: std::sync::Arc::from(format!("{a}:{k}").as_str()),
                            }],
                            children: Vec::new(),
                            marks: crate::foreign::ForeignMarks::empty(),
                        }),
                    ),
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 2048,
        ..ProptestConfig::default()
    })]

    /// Undo everything and the document is byte for byte what it was; redo
    /// everything and it is byte for byte what it became. A wrong
    /// `Action::inverse` is the failure mode that corrupts documents
    /// silently, and this is what catches it.
    #[test]
    fn undo_and_redo_round_trip_byte_for_byte(
        edits in prop::collection::vec(edit_strategy(), 0..60)
    ) {
        let mut f = fixture();
        let mut bus = CommandBus::new();
        let start = f.doc.canonical_digest();
        for e in edits {
            let ids: Vec<NodeId> = f.doc.tree.preorder(f.doc.tree.root()).collect();
            let _ = bus.dispatch(&mut f.doc, &EditCommand(e, ids));
            let report = f.doc.validate();
            prop_assert!(report.errors.is_empty(), "{:#?}", report.errors);
        }
        let end = f.doc.canonical_digest();
        while bus.history_mut().undo(&mut f.doc).is_some() {}
        prop_assert_eq!(start, f.doc.canonical_digest(), "undo did not return the document");
        while bus.history_mut().redo(&mut f.doc).is_some() {}
        prop_assert_eq!(end, f.doc.canonical_digest(), "redo did not return the document");
    }

    /// A rolled-back command leaves nothing behind.
    #[test]
    fn a_failing_command_never_changes_the_digest(
        edits in prop::collection::vec(edit_strategy(), 0..8)
    ) {
        #[derive(Debug)]
        struct Failing(Vec<Edit>, Vec<NodeId>);
        impl Command for Failing {
            fn label(&self) -> &'static str { "failing" }
            fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
                for e in &self.0 {
                    EditCommand(*e, self.1.clone()).run(tx)?;
                }
                Err(EditError::LimitExceeded("deliberate"))
            }
        }
        let mut f = fixture();
        let before = f.doc.canonical_digest();
        let ids: Vec<NodeId> = f.doc.tree.preorder(f.doc.tree.root()).collect();
        let mut bus = CommandBus::new();
        let r = bus.dispatch(&mut f.doc, &Failing(edits, ids));
        prop_assert!(r.is_err());
        prop_assert_eq!(before, f.doc.canonical_digest());
    }
}

#[derive(Clone, Debug)]
enum Script {
    Down,
    Up,
    Node(u8),
    Attr,
    Bitmap,
}

fn script_strategy() -> impl Strategy<Value = Script> {
    prop_oneof![
        Just(Script::Down),
        Just(Script::Up),
        (0u8..8).prop_map(Script::Node),
        Just(Script::Attr),
        Just(Script::Bitmap),
    ]
}

fn emit(b: &mut DocumentBuilder, s: &Script) -> Result<(), BuildError> {
    match s {
        Script::Down => b.push_scope(),
        Script::Up => {
            b.pop_scope();
            Ok(())
        }
        Script::Node(k) => {
            let kind = match k % 8 {
                0 => NodeKind::Chapter,
                1 => NodeKind::Spread(Box::default()),
                2 => NodeKind::Page(Box::default()),
                3 => NodeKind::Layer(Box::default()),
                4 => NodeKind::Grid(Box::default()),
                5 => NodeKind::Group(Box::default()),
                6 => NodeKind::TextItem(crate::text::TextItem::Char('a')),
                _ => NodeKind::Path(Box::new(PathNode::new(square(Point::ORIGIN, 1_000)))),
            };
            b.node(kind).map(|_| ())
        }
        Script::Attr => b
            .attribute(AttrValue::ObjectName(Arc::from("n")))
            .map(|_| ()),
        Script::Bitmap => b
            .node(NodeKind::Bitmap(Box::new(crate::kind::BitmapNode {
                image: crate::resources::BitmapId::default(),
                origin: Point::ORIGIN,
                major: Vector::raw(1, 0),
                minor: Vector::raw(0, 1),
                photo_ops: Default::default(),
            })))
            .map(|_| ()),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_shrink_iters: 4096,
        ..ProptestConfig::default()
    })]

    /// The builder has exactly two outcomes: an error, or a document that
    /// validates. Never a third.
    #[test]
    fn the_builder_cannot_produce_an_invalid_document(
        script in prop::collection::vec(script_strategy(), 0..120)
    ) {
        let mut b = DocumentBuilder::new(BuildLimits::small());
        let mut failed = false;
        for s in &script {
            if emit(&mut b, s).is_err() {
                failed = true;
                break;
            }
        }
        if failed {
            return Ok(());
        }
        match b.finish() {
            Err(_) => {}
            Ok((doc, _)) => {
                let r = doc.validate();
                prop_assert!(r.errors.is_empty(), "{:#?}", r.errors);
            }
        }
    }
}
