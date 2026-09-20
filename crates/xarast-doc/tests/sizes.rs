//! The size gates. When one of these fails, the fix is to box a variant,
//! never to raise the limit.

use std::mem::size_of;

use xarast_doc::{AttrValue, NodeData, NodeId, NodeKind, TextItem};

#[test]
fn node_data_fits_in_a_cache_line() {
    let n = size_of::<NodeData>();
    assert!(
        n <= 64,
        "NodeData is {n} bytes, over the 64-byte gate. \
         NodeKind is {} bytes; box its largest variant rather than raising \
         the limit.",
        size_of::<NodeKind>()
    );
}

#[test]
fn option_node_id_uses_the_niche() {
    assert_eq!(
        size_of::<Option<NodeId>>(),
        size_of::<NodeId>(),
        "the Links design assumes Option<NodeId> is free"
    );
}

#[test]
fn node_kind_is_two_words() {
    assert!(
        size_of::<NodeKind>() <= 16,
        "NodeKind is {} bytes; every payload over a pointer must be boxed",
        size_of::<NodeKind>()
    );
}

#[test]
fn a_text_item_costs_nothing_extra() {
    // A document full of text is mostly these; they must not pay for a
    // spread.
    assert!(size_of::<TextItem>() <= 8, "{}", size_of::<TextItem>());
}

#[test]
fn attr_value_is_only_ever_behind_a_box() {
    // No gate on the value itself; the gate is that `NodeKind::Attr` boxes it.
    // This test documents the size so a jump shows up in review.
    assert!(
        size_of::<AttrValue>() <= 256,
        "AttrValue grew to {} bytes",
        size_of::<AttrValue>()
    );
}
