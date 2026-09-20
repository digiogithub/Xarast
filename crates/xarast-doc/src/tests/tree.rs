//! The arena and its traversals.

use crate::Document;
use crate::kind::NodeKind;
use crate::tests::{fixture, layer, path_node, square};
use crate::tree::{Attach, NodeFlags, Tag};
use crate::walk::{Descend, WalkEvent};
use xarast_geom::Point;

#[test]
fn the_canonical_document_validates() {
    let doc = Document::new_empty();
    doc.validate().assert_clean();
    assert!(doc.active_layer(doc.active_spread()).is_some());
}

#[test]
fn attach_at_every_anchor_position() {
    for how in [
        Attach::FirstChild,
        Attach::LastChild,
        Attach::Prev,
        Attach::Next,
    ] {
        let mut f = fixture();
        let n = f.doc.tree.create(layer("extra"));
        let anchor = match how {
            Attach::FirstChild | Attach::LastChild => f.layer,
            Attach::Prev | Attach::Next => f.path,
        };
        f.doc.tree.attach(n, anchor, how).unwrap();
        f.doc.validate().assert_clean();
        assert!(f.doc.tree.is_reachable(n));
        let (a, h) = f.doc.tree.anchor_of(n).unwrap();
        // Re-attaching at the reported anchor puts it back where it was.
        f.doc.tree.detach(n).unwrap();
        f.doc.tree.attach(n, a, h).unwrap();
        f.doc.validate().assert_clean();
    }
}

#[test]
fn anchor_of_round_trips_for_every_node() {
    let mut f = fixture();
    let ids: Vec<_> = f.doc.tree.preorder(f.doc.tree.root()).skip(1).collect();
    let before = f.doc.canonical_digest();
    for id in ids {
        let Some((a, h)) = f.doc.tree.anchor_of(id) else {
            continue;
        };
        f.doc.tree.detach(id).unwrap();
        f.doc.tree.attach(id, a, h).unwrap();
    }
    assert_eq!(before, f.doc.canonical_digest());
}

#[test]
fn detaching_keeps_the_node_alive() {
    let mut f = fixture();
    f.doc.tree.detach(f.group).unwrap();
    assert!(f.doc.tree.contains(f.group));
    assert!(f.doc.tree.contains(f.shape_a));
    assert!(!f.doc.tree.is_reachable(f.group));
    assert!(
        f.doc.tree[f.group].flags.contains(NodeFlags::DETACHED),
        "detach must set the flag; there is no NodeHidden any more"
    );
    f.doc.validate().assert_clean();
}

#[test]
fn destroying_a_subtree_removes_every_node_and_its_tag() {
    let mut f = fixture();
    let tag = f.doc.tree[f.shape_a].tag;
    let n = f.doc.tree.destroy_subtree(f.group);
    assert_eq!(n, 4, "the group, its fill and its two shapes");
    assert!(!f.doc.tree.contains(f.shape_a));
    assert_eq!(f.doc.tree.by_tag(tag), None);
    f.doc.validate().assert_clean();
}

#[test]
fn the_root_cannot_be_detached_or_destroyed() {
    let mut doc = Document::new_empty();
    let root = doc.tree.root();
    assert!(doc.tree.detach(root).is_err());
    assert_eq!(doc.tree.destroy_subtree(root), 0);
}

#[test]
fn a_cycle_is_refused_rather_than_created() {
    let mut f = fixture();
    f.doc.tree.detach(f.group).unwrap();
    let err = f.doc.tree.attach(f.group, f.shape_a, Attach::LastChild);
    assert!(err.is_err(), "attaching a node under its own child");
}

#[test]
fn tags_are_unique_and_resolve() {
    let f = fixture();
    let ids: Vec<_> = f.doc.tree.preorder(f.doc.tree.root()).collect();
    for id in ids {
        let tag = f.doc.tree[id].tag;
        assert_eq!(f.doc.tree.by_tag(tag), Some(id));
    }
    assert_eq!(f.doc.tree.by_tag(Tag(u32::MAX)), None);
}

#[test]
fn preorder_is_parent_first_and_postorder_is_children_first() {
    let f = fixture();
    let pre: Vec<_> = f.doc.tree.preorder(f.group).collect();
    assert_eq!(pre, vec![f.group, f.group_fill, f.shape_a, f.shape_b]);
    let post: Vec<_> = f.doc.tree.postorder(f.group).collect();
    assert_eq!(post, vec![f.group_fill, f.shape_a, f.shape_b, f.group]);
}

#[test]
fn children_iterates_both_ways() {
    let f = fixture();
    let fwd: Vec<_> = f.doc.tree.children(f.group).collect();
    let mut back: Vec<_> = f.doc.tree.children(f.group).rev().collect();
    back.reverse();
    assert_eq!(fwd, back);
}

#[test]
fn ancestors_walks_up_to_the_root() {
    let f = fixture();
    let anc: Vec<_> = f.doc.tree.ancestors(f.shape_a).collect();
    assert_eq!(anc.first(), Some(&f.group));
    assert_eq!(anc.last(), Some(&f.doc.tree.root()));
    assert_eq!(f.doc.tree.depth_of(f.shape_a), anc.len());
}

#[test]
fn walk_render_brackets_every_child_list() {
    let f = fixture();
    let events: Vec<_> = f.doc.tree.walk_render(f.group).collect();
    assert_eq!(
        events,
        vec![
            WalkEvent::Visit { node: f.group },
            WalkEvent::EnterScope { parent: f.group },
            WalkEvent::Visit { node: f.group_fill },
            WalkEvent::Visit { node: f.shape_a },
            WalkEvent::Visit { node: f.shape_b },
            WalkEvent::LeaveScope { parent: f.group },
        ]
    );
}

#[test]
fn walk_render_scopes_are_balanced_over_the_whole_document() {
    let f = fixture();
    let mut depth = 0i32;
    for ev in f.doc.tree.walk_render(f.doc.tree.root()) {
        match ev {
            WalkEvent::EnterScope { .. } => depth += 1,
            WalkEvent::LeaveScope { .. } => depth -= 1,
            WalkEvent::Visit { .. } => {}
        }
        assert!(depth >= 0);
    }
    assert_eq!(depth, 0);
}

#[test]
fn descend_skip_prunes_the_child_list() {
    let f = fixture();
    let mut walk = f.doc.tree.walk_render(f.layer);
    let mut visited = Vec::new();
    while let Some(ev) = walk.next() {
        if let WalkEvent::Visit { node } = ev {
            visited.push(node);
            if node == f.group {
                walk.control(Descend::Skip);
            }
        }
    }
    assert!(visited.contains(&f.group));
    assert!(!visited.contains(&f.shape_a), "the group was pruned");
}

#[test]
fn descend_jump_to_resumes_at_a_later_node() {
    let f = fixture();
    let mut walk = f.doc.tree.walk_render(f.group);
    let mut visited = Vec::new();
    while let Some(ev) = walk.next() {
        if let WalkEvent::Visit { node } = ev {
            visited.push(node);
            if node == f.group {
                walk.control(Descend::JumpTo(f.shape_b));
            }
        }
    }
    assert_eq!(visited, vec![f.group, f.shape_b]);
}

#[test]
fn descend_run_to_still_reports_attributes() {
    let f = fixture();
    let mut walk = f.doc.tree.walk_render(f.group);
    let mut visited = Vec::new();
    while let Some(ev) = walk.next() {
        if let WalkEvent::Visit { node } = ev {
            visited.push(node);
            if node == f.group {
                walk.control(Descend::RunTo(f.shape_b));
            }
        }
    }
    assert_eq!(
        visited,
        vec![f.group, f.group_fill, f.shape_b],
        "RunTo keeps the attribute stack correct, so attributes still come out"
    );
}

#[test]
fn bounds_invalidation_climbs_and_stops_early() {
    let mut f = fixture();
    f.doc.update_bounds();
    assert!(f.doc.tree.bounds(f.group).is_valid());
    assert!(f.doc.tree.bounds(f.layer).is_valid());
    f.doc.tree.invalidate_bounds(f.shape_a);
    assert!(!f.doc.tree.bounds(f.shape_a).is_valid());
    assert!(!f.doc.tree.bounds(f.group).is_valid());
    assert!(!f.doc.tree.bounds(f.layer).is_valid());
    f.doc.validate().assert_clean();
}

#[test]
fn bounds_cover_the_geometry() {
    let mut doc = Document::new_empty();
    let spread = doc.active_spread();
    let l = doc.active_layer(spread).unwrap();
    let p = doc
        .tree
        .create(path_node(square(Point::raw(1_000, 2_000), 10_000)));
    doc.tree.attach(p, l, Attach::LastChild).unwrap();
    doc.update_bounds();
    let b = doc.tree.bounds(p).get().expect("a path has a box");
    assert!(b.lo.x.raw() <= 1_000 && b.hi.x.raw() >= 11_000);
}

#[test]
fn a_detached_subtree_is_invisible_to_traversal_from_the_root() {
    let mut f = fixture();
    f.doc.tree.detach(f.group).unwrap();
    let seen: Vec<_> = f.doc.tree.preorder(f.doc.tree.root()).collect();
    assert!(!seen.contains(&f.group));
    assert!(!seen.contains(&f.shape_a));
}

#[test]
fn node_kind_predicates_agree_with_each_other() {
    let f = fixture();
    for id in f.doc.tree.preorder(f.doc.tree.root()) {
        let k = &f.doc.tree[id].kind;
        assert!(
            !(k.is_ink() && k.is_paper()),
            "{} cannot be both ink and paper",
            k.type_name()
        );
        if k.is_attr() {
            assert!(!k.is_ink() && !k.is_paper());
        }
        if matches!(k, NodeKind::Group(_)) {
            assert!(k.is_compound() && k.is_ink());
        }
    }
}
