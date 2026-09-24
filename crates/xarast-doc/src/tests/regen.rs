//! Live objects: structure helpers, regeneration, the queue and the cache
//! (phase 13, A1–A4).

use std::cell::RefCell;

use xarast_geom::{BiasGain, Mp, Point};

use crate::builder::{BuildLimits, skeleton};
use crate::history::{Command, CommandBus, EditError, Tx};
use crate::kind::NodeKind;
use crate::live::{
    LiveKind, LiveNode, LiveRole, RegenState, ShadowKind, ShadowParams, controller_of,
    in_generated, parts,
};
use crate::regen::{LiveCache, LiveOutput, RegenError, RegenQueue, regen_key, regenerate};
use crate::tests::{black_fill, path_node, square};
use crate::tree::NodeId;
use crate::{Document, attr::AttrValue};

fn shadow(role: LiveRole, blur: i32) -> NodeKind {
    NodeKind::Live(Box::new(LiveNode {
        role,
        kind: LiveKind::Shadow(Box::new(ShadowParams {
            kind: ShadowKind::Wall,
            offset: Point::raw(1000, -1000),
            blur: Mp::new(blur),
            darkness: 0.5,
            profile: BiasGain::IDENTITY,
            scale: 1.0,
            tilt: 0.0,
        })),
        regen: RegenState::Clean,
        name: None,
    }))
}

/// A layer holding an outer shadow whose source holds a path and an inner
/// shadow (source: a path; generated: a path a file stored), and a plain
/// path beside it.
fn nested() -> Document {
    let mut b = skeleton(BuildLimits::small()).unwrap();
    b.node(shadow(LiveRole::Controller, 4000)).unwrap();
    b.push_scope().unwrap();
    b.node(shadow(LiveRole::Source, 4000)).unwrap();
    b.push_scope().unwrap();
    b.attribute(black_fill()).unwrap();
    b.node(path_node(square(Point::ORIGIN, 1000))).unwrap();
    b.node(shadow(LiveRole::Controller, 2000)).unwrap();
    b.push_scope().unwrap();
    b.node(shadow(LiveRole::Source, 2000)).unwrap();
    b.push_scope().unwrap();
    b.node(path_node(square(Point::raw(5000, 0), 1000)))
        .unwrap();
    b.pop_scope();
    b.node(shadow(LiveRole::Generated, 2000)).unwrap();
    b.push_scope().unwrap();
    b.node(path_node(square(Point::raw(6000, -1000), 1000)))
        .unwrap();
    b.pop_scope();
    b.pop_scope();
    b.pop_scope();
    b.pop_scope();
    b.node(path_node(square(Point::raw(20_000, 0), 1000)))
        .unwrap();
    let (doc, _) = b.finish().unwrap();
    doc.validate().assert_clean();
    doc
}

fn controllers(doc: &Document) -> Vec<NodeId> {
    doc.tree
        .preorder(doc.tree.root())
        .filter(|n| {
            matches!(doc.tree.kind(*n), Some(NodeKind::Live(l)) if l.role == LiveRole::Controller)
        })
        .collect()
}

fn paths(doc: &Document) -> Vec<NodeId> {
    doc.tree
        .preorder(doc.tree.root())
        .filter(|n| matches!(doc.tree.kind(*n), Some(NodeKind::Path(_))))
        .collect()
}

#[derive(Debug)]
struct SetBlur(NodeId, i32);

impl Command for SetBlur {
    fn label(&self) -> &'static str {
        "Shadow blur"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let mut k = tx
            .doc()
            .tree
            .kind(self.0)
            .cloned()
            .ok_or(EditError::NotPermitted(self.0))?;
        if let NodeKind::Live(l) = &mut k
            && let LiveKind::Shadow(p) = &mut l.kind
        {
            p.blur = Mp::new(self.1);
        }
        tx.set_kind(self.0, k)
    }
}

#[derive(Debug)]
struct Delete(NodeId);

impl Command for Delete {
    fn label(&self) -> &'static str {
        "Delete"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.delete(self.0)
    }
}

#[test]
fn a_controller_names_its_parts_and_its_generated_data_needs_a_parent() {
    let doc = nested();
    let c = controllers(&doc);
    let (outer, inner) = (c[0], c[1]);
    let p = parts(&doc.tree, inner).expect("a controller with a source");
    assert!(p.generated.is_some());
    assert!(parts(&doc.tree, outer).unwrap().generated.is_none());
    let ps = paths(&doc);
    // The inner source's path, the stored generated path, the plain path.
    assert_eq!(controller_of(&doc.tree, ps[1]), Some(inner));
    assert_eq!(controller_of(&doc.tree, ps[0]), Some(outer));
    assert_eq!(controller_of(&doc.tree, ps[3]), None);
    assert!(in_generated(&doc.tree, ps[2]));
    assert!(!in_generated(&doc.tree, ps[1]));
    assert!(LiveRole::Generated.needs_parent());
    assert!(!LiveRole::Source.needs_parent());
    assert!(parts(&doc.tree, ps[0]).is_none());
}

#[test]
fn nothing_inside_a_generated_subtree_is_edited_on_its_own() {
    let mut doc = nested();
    let mut bus = CommandBus::new();
    let stored = paths(&doc)[2];
    let r = bus.dispatch(&mut doc, &Delete(stored));
    assert!(matches!(r, Err(EditError::NotPermitted(_))), "{r:?}");
    assert!(!bus.history().can_undo());
    // The controller, with everything it holds, can go.
    let inner = controllers(&doc)[1];
    bus.dispatch(&mut doc, &Delete(inner)).unwrap();
    doc.validate().assert_clean();
}

#[test]
fn regenerating_reads_the_document_and_every_kind_answers() {
    let doc = nested();
    let before = doc.canonical_digest();
    for c in controllers(&doc) {
        assert_eq!(regenerate(&doc, c, 96.0), Ok(LiveOutput::Stored));
        // Idempotent for an unchanged state.
        assert_eq!(regen_key(&doc, c, 96.0), regen_key(&doc, c, 96.0));
    }
    let plain = paths(&doc)[3];
    assert_eq!(
        regenerate(&doc, plain, 96.0),
        Err(RegenError::NotAController(plain))
    );
    assert_eq!(regen_key(&doc, plain, 96.0), None);
    assert_eq!(doc.canonical_digest(), before, "regeneration wrote nothing");
}

#[test]
fn the_key_follows_params_source_inheritance_and_resolution_but_not_the_rest() {
    let mut doc = nested();
    let mut bus = CommandBus::new();
    let c = controllers(&doc);
    let (outer, inner) = (c[0], c[1]);
    let k0 = (regen_key(&doc, outer, 96.0), regen_key(&doc, inner, 96.0));
    assert_ne!(regen_key(&doc, outer, 96.0), regen_key(&doc, outer, 192.0));
    assert_eq!(
        regen_key(&doc, outer, 96.0),
        regen_key(&doc, outer, 96.001),
        "a sub-quantum zoom does not regenerate"
    );
    // A parameter of the inner controller: both keys move (the inner is in
    // the outer's source).
    bus.dispatch(&mut doc, &SetBlur(inner, 3000)).unwrap();
    let k1 = (regen_key(&doc, outer, 96.0), regen_key(&doc, inner, 96.0));
    assert_ne!(k1.0, k0.0);
    assert_ne!(k1.1, k0.1);
    // The plain path beside them: neither moves.
    let plain = paths(&doc)[3];
    bus.dispatch(&mut doc, &Delete(plain)).unwrap();
    assert_eq!(
        (regen_key(&doc, outer, 96.0), regen_key(&doc, inner, 96.0)),
        k1
    );
    // Undo comes back to a state with the keys of that state.
    bus.history_mut().undo(&mut doc).unwrap();
    bus.history_mut().undo(&mut doc).unwrap();
    // Content revisions move forward on undo too (document-model decision
    // 34), so the key is new: undo regenerates, it never draws stale.
    assert_ne!(regen_key(&doc, inner, 96.0), k1.1);
}

#[test]
fn one_parameter_change_is_one_undo_step_and_a_flush_records_nothing() {
    let mut doc = nested();
    let mut bus = CommandBus::new();
    let inner = controllers(&doc)[1];
    let mut q = RegenQueue::new();
    let mut cache = LiveCache::new();
    bus.dispatch(&mut doc, &SetBlur(inner, 3000)).unwrap();
    let digest = doc.canonical_digest();
    let log = doc.tree.drain_changes();
    q.mark_changes(&doc.tree, &log);
    let report = q.flush(&doc, 96.0, &mut cache, &mut regenerate);
    assert_eq!(report.regenerated.len(), 2, "{report:?}");
    assert_eq!(doc.canonical_digest(), digest);
    assert_eq!(bus.history().len(), 1);
    bus.history_mut().undo(&mut doc).unwrap();
    assert!(!bus.history().can_undo());
}

#[test]
fn the_queue_deduplicates_and_flushes_deepest_first_once_per_state() {
    let mut doc = nested();
    let mut bus = CommandBus::new();
    let c = controllers(&doc);
    let (outer, inner) = (c[0], c[1]);
    let mut q = RegenQueue::new();
    let mut cache = LiveCache::new();
    // A drag: many changes to the inner controller, one flush.
    let g = bus.begin_gesture();
    for blur in [2100, 2200, 2300, 2400] {
        bus.dispatch(&mut doc, &SetBlur(inner, blur)).unwrap();
    }
    bus.end_gesture(g);
    let log = doc.tree.drain_changes();
    q.mark_changes(&doc.tree, &log);
    q.mark(inner, RegenState::Deferred);
    assert_eq!(q.len(), 2, "the inner and the outer, once each");
    let calls = RefCell::new(Vec::new());
    let mut counting = |d: &Document, id: NodeId, dpi: f64| {
        calls.borrow_mut().push(id);
        regenerate(d, id, dpi)
    };
    let r = q.flush(&doc, 96.0, &mut cache, &mut counting);
    assert_eq!(*calls.borrow(), vec![inner, outer], "deepest first");
    assert_eq!(r.regenerated, vec![inner, outer]);
    assert!(q.is_empty());
    assert!(cache.get(inner).is_some());
    // Nothing changed since: marked again, found current, not recomputed.
    q.mark(inner, RegenState::Deferred);
    q.mark(outer, RegenState::Deferred);
    let r = q.flush(&doc, 96.0, &mut cache, &mut counting);
    assert_eq!(r.unchanged, 2);
    assert_eq!(calls.borrow().len(), 2);
    // A new zoom is a new state.
    q.mark(outer, RegenState::Deferred);
    let r = q.flush(&doc, 150.0, &mut cache, &mut counting);
    assert_eq!(r.regenerated, vec![outer]);
}

#[test]
fn urgent_entries_flush_alone_and_failures_are_not_drawn_stale() {
    let mut doc = nested();
    let c = controllers(&doc);
    let (outer, inner) = (c[0], c[1]);
    let mut q = RegenQueue::new();
    let mut cache = LiveCache::new();
    q.mark(outer, RegenState::Deferred);
    q.mark(inner, RegenState::Dirty);
    q.mark(inner, RegenState::Deferred);
    let r = q.flush_urgent(&doc, 96.0, &mut cache, &mut regenerate);
    assert_eq!(
        r.regenerated,
        vec![inner],
        "Dirty survives a later Deferred mark"
    );
    assert_eq!(q.len(), 1);
    // A generator that fails drops what was cached.
    q.mark(inner, RegenState::Dirty);
    let mut failing = |_: &Document, id: NodeId, _: f64| Err(RegenError::NoSource(id));
    // The inner key is unchanged, so its cached output is still current
    // and the generator is not even asked; the outer one, still deferred
    // from before, has nothing cached and fails.
    let r = q.flush(&doc, 96.0, &mut cache, &mut failing);
    assert_eq!((r.unchanged, r.failed.len()), (1, 1));
    assert_eq!(r.failed[0].0, outer);
    assert!(cache.get(inner).is_some());
    let r = {
        q.mark(inner, RegenState::Dirty);
        q.flush(&doc, 300.0, &mut cache, &mut failing)
    };
    assert_eq!(r.failed.len(), 1);
    assert!(cache.get(inner).is_none());
    // A deleted controller is skipped and forgotten.
    let mut bus = CommandBus::new();
    q.flush(&doc, 96.0, &mut cache, &mut regenerate);
    bus.dispatch(&mut doc, &Delete(outer)).unwrap();
    q.mark(outer, RegenState::Deferred);
    let r = q.flush(&doc, 96.0, &mut cache, &mut regenerate);
    assert_eq!(r.skipped, 1);
    assert!(cache.get(outer).is_none());
    bus.history_mut().clear(&mut doc);
    cache.retain_alive(&doc.tree);
    assert!(cache.is_empty());
}

#[test]
fn an_attribute_the_controller_inherits_changes_its_key() {
    let mut doc = nested();
    let mut bus = CommandBus::new();
    let outer = controllers(&doc)[0];
    let k = regen_key(&doc, outer, 96.0);
    let layer = doc.tree.ancestors(outer).next().unwrap();
    #[derive(Debug)]
    struct AddFill(NodeId);
    impl Command for AddFill {
        fn label(&self) -> &'static str {
            "Fill"
        }
        fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
            crate::fill_edit::set_own_attr(tx, self.0, AttrValue::LineWidth(Mp::new(500)))
        }
    }
    bus.dispatch(&mut doc, &AddFill(layer)).unwrap();
    assert_ne!(regen_key(&doc, outer, 96.0), k);
}

#[test]
fn a_procedural_bitmap_is_keyed_by_every_parameter_and_its_generator() {
    use crate::fill::ProceduralParams;
    use crate::resources::ProceduralSource;
    let a = ProceduralSource {
        params: ProceduralParams::default(),
        fractal: true,
    };
    assert_eq!(a.cache_key(), a.clone().cache_key());
    let mut b = a.clone();
    b.params.seed = 1;
    assert_ne!(a.cache_key(), b.cache_key());
    let mut c = a.clone();
    c.params.graininess = f32::from_bits(a.params.graininess.to_bits() + 1);
    assert_ne!(a.cache_key(), c.cache_key(), "one ulp is another bitmap");
    let d = ProceduralSource {
        fractal: false,
        ..a.clone()
    };
    assert_ne!(a.cache_key(), d.cache_key());
}
