//! The attribute stack, the resolver and their agreement.

use std::sync::Arc;

use crate::Document;
use crate::attr::{
    ALL_ATTR_SLOTS, ATTR_SLOT_COUNT, AttrSlot, AttrStack, AttrValue, DefaultAttrs, default_for,
    resolve_uncached,
};
use crate::kind::NodeKind;
use crate::tests::{black_fill, fixture, white_fill};
use crate::tree::{Attach, NodeId};
use crate::walk::WalkEvent;
use xarast_geom::Mp;

#[test]
fn every_slot_has_a_default_and_the_default_belongs_to_it() {
    let d = DefaultAttrs::xara_compatible();
    assert_eq!(ALL_ATTR_SLOTS.len(), ATTR_SLOT_COUNT);
    for s in ALL_ATTR_SLOTS {
        assert_eq!(
            d.get(s).slot(),
            Some(s),
            "the default for {s:?} belongs to another slot"
        );
        assert_eq!(default_for(s).slot(), Some(s));
    }
}

#[test]
fn the_slot_mapping_is_total_and_injective() {
    // Every slot is reachable from exactly one value, which is what makes the
    // dense table well defined and what the .xar tag mapping is checked
    // against.
    let mut seen = [false; ATTR_SLOT_COUNT];
    for s in ALL_ATTR_SLOTS {
        let i = s as usize;
        assert!(!seen[i], "{s:?} appears twice in ALL_ATTR_SLOTS");
        seen[i] = true;
    }
    assert!(seen.iter().all(|x| *x), "a slot is missing from the table");
}

#[test]
fn multi_applicable_attributes_occupy_no_slot() {
    let name = AttrValue::ObjectName(Arc::from("badge"));
    assert_eq!(name.slot(), None);
    let user = AttrValue::User(crate::attr::MultiAttr {
        key: Arc::from("k"),
        value: Arc::from("v"),
    });
    assert_eq!(user.slot(), None);

    let d = DefaultAttrs::xara_compatible();
    let mut s = AttrStack::with_defaults(&d);
    s.push(Arc::new(name));
    s.push(Arc::new(user));
    assert_eq!(s.multi().len(), 2, "they accumulate instead of replacing");
}

#[test]
fn push_scope_and_pop_scope_restore_exactly() {
    let d = DefaultAttrs::xara_compatible();
    let mut s = AttrStack::with_defaults(&d);
    let before = s.snapshot();
    s.push_scope();
    s.push(Arc::new(AttrValue::LineWidth(Mp::new(9_000))));
    s.push(Arc::new(black_fill()));
    s.push(Arc::new(AttrValue::ObjectName(Arc::from("x"))));
    assert_eq!(
        s.get(AttrSlot::LineWidth),
        &AttrValue::LineWidth(Mp::new(9_000))
    );
    assert_eq!(s.multi().len(), 1);
    s.pop_scope();
    assert_eq!(s.multi().len(), 0);
    assert!(before == s.snapshot(), "pop_scope must restore exactly");
}

#[test]
fn an_unbalanced_pop_scope_does_not_panic() {
    let d = DefaultAttrs::xara_compatible();
    let mut s = AttrStack::with_defaults(&d);
    s.pop_scope();
    s.pop_scope();
    assert_eq!(s.depth(), 0);
}

#[test]
fn an_attribute_applies_to_its_following_siblings_and_to_its_parents_ink() {
    let f = fixture();
    let d = &f.doc.defaults;
    // The layer's black fill reaches the path that follows it.
    let at_path = resolve_uncached(&f.doc.tree, f.path, d);
    assert_eq!(at_path.get(AttrSlot::FillGeometry), &black_fill());
    // The group's own white fill wins inside the group.
    let at_shape = resolve_uncached(&f.doc.tree, f.shape_a, d);
    assert_eq!(at_shape.get(AttrSlot::FillGeometry), &white_fill());
    // And it reaches the group's own ink, because a parent paints last.
    let at_group = resolve_uncached(&f.doc.tree, f.group, d);
    assert_eq!(at_group.get(AttrSlot::FillGeometry), &white_fill());
}

#[test]
fn an_attribute_does_not_leak_out_of_its_scope() {
    let mut f = fixture();
    // A path after the group must see the layer's fill, not the group's.
    let after = f
        .doc
        .tree
        .create(crate::tests::path_node(crate::tests::square(
            xarast_geom::Point::ORIGIN,
            1_000,
        )));
    f.doc
        .tree
        .attach(after, f.layer, Attach::LastChild)
        .unwrap();
    let r = resolve_uncached(&f.doc.tree, after, &f.doc.defaults);
    assert_eq!(r.get(AttrSlot::FillGeometry), &black_fill());
}

/// Walks with an [`AttrStack`] exactly as a renderer would, and returns what
/// was in force at each node's paint point.
fn walk_resolution(doc: &Document) -> Vec<(NodeId, crate::attr::ResolvedAttrs)> {
    let mut stack = AttrStack::with_defaults(&doc.defaults);
    let mut out = Vec::new();
    for ev in doc.tree.walk_render(doc.tree.root()) {
        match ev {
            WalkEvent::EnterScope { .. } => stack.push_scope(),
            WalkEvent::Visit { node } => match doc.tree.kind(node) {
                Some(NodeKind::Attr(a)) => stack.push(Arc::new(a.value.clone())),
                _ => {
                    if doc.tree.links(node).first_child.is_none() {
                        out.push((node, stack.snapshot()));
                    }
                }
            },
            WalkEvent::LeaveScope { parent } => {
                out.push((parent, stack.snapshot()));
                stack.pop_scope();
            }
        }
    }
    out
}

#[test]
fn the_walk_and_the_uncached_resolver_agree_everywhere() {
    let f = fixture();
    for (node, walked) in walk_resolution(&f.doc) {
        let resolved = resolve_uncached(&f.doc.tree, node, &f.doc.defaults);
        assert!(
            walked == resolved,
            "the stack walk and the resolver disagree at {node:?}"
        );
    }
}

#[test]
fn the_cache_agrees_with_the_uncached_resolver_and_survives_an_edit() {
    let mut f = fixture();
    let nodes: Vec<_> = f.doc.tree.preorder(f.doc.tree.root()).collect();
    for n in &nodes {
        let expected = resolve_uncached(&f.doc.tree, *n, &f.doc.defaults);
        let defaults = f.doc.defaults.clone();
        let got = f.doc.attrs.resolve(&f.doc.tree, *n, &defaults).clone();
        assert!(got == expected, "cache disagrees at {n:?}");
    }
    // Change an attribute, invalidate, and check it agrees again.
    if let Some(NodeKind::Attr(a)) = f.doc.tree.get_mut(f.group_fill).map(|n| &mut n.kind) {
        a.value = AttrValue::Fill(match black_fill() {
            AttrValue::Fill(p) => p,
            _ => unreachable!(),
        });
    }
    f.doc.attrs.invalidate_subtree(&f.doc.tree, f.group);
    for n in &nodes {
        let expected = resolve_uncached(&f.doc.tree, *n, &f.doc.defaults);
        let defaults = f.doc.defaults.clone();
        let got = f.doc.attrs.resolve(&f.doc.tree, *n, &defaults).clone();
        assert!(got == expected, "cache is stale at {n:?} after an edit");
    }
}

#[test]
fn stroke_extent_grows_with_the_line_width_and_the_feather() {
    let d = DefaultAttrs::xara_compatible();
    let mut s = AttrStack::with_defaults(&d);
    s.push(Arc::new(AttrValue::LineWidth(Mp::new(10_000))));
    s.push(Arc::new(AttrValue::JoinType(xarast_geom::Join::Round)));
    let thin = s.snapshot().stroke_extent();
    assert_eq!(thin, Mp::new(5_000));
    s.push(Arc::new(AttrValue::Feather {
        size: Mp::new(1_000),
        profile: xarast_geom::BiasGain::IDENTITY,
    }));
    assert_eq!(s.snapshot().stroke_extent(), Mp::new(6_000));
}

#[test]
fn only_geometry_linked_attributes_claim_to_be() {
    assert!(
        !black_fill().linked_to_geometry(),
        "a flat fill has no points"
    );
    let grad = AttrValue::Fill(crate::fill::Paint::Linear {
        start: xarast_geom::Point::ORIGIN,
        end: xarast_geom::Point::raw(1000, 0),
        persp: None,
        from: xarast_color::Colour::Direct(xarast_color::ColourValue::rgbt(0.0, 0.0, 0.0, 0.0)),
        to: xarast_color::Colour::Direct(xarast_color::ColourValue::rgbt(1.0, 1.0, 1.0, 0.0)),
        ramp: crate::fill::Ramp::new(),
    });
    assert!(grad.linked_to_geometry());
    assert!(!grad.affects_bounds());
    assert!(
        AttrValue::Feather {
            size: Mp::new(1),
            profile: xarast_geom::BiasGain::IDENTITY
        }
        .is_effect()
    );
}
