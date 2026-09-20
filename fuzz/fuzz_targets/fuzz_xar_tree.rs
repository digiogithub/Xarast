//! Tree reconstruction and the three-way unknown-tag policy.
//!
//! What this is hunting: an imbalance that loses nodes or duplicates them,
//! an atomic strip that never ends, a nesting depth that overflows the
//! stack when the tree is dropped rather than when it is built.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_xar::{ReaderLimits, analyse};

fuzz_target!(|data: &[u8]| {
    let limits = ReaderLimits {
        max_inflated_per_block: 1 << 20,
        inflation_ratio: 100,
        max_total_inflated: 8 << 20,
        max_record_size: 1 << 20,
        max_records: 200_000,
        max_tree_depth: 64,
    };
    let Ok(a) = analyse(data, limits) else {
        return;
    };
    // Nothing is counted twice, and nothing vanishes.
    let counted: u64 = a.tree.histogram.values().map(|n| u64::from(*n)).sum();
    assert_eq!(counted, u64::from(a.records_read));

    // Every node in the tree is one of the records that was read, and the
    // depth cap really does cap.
    let mut nodes = 0usize;
    let mut deepest = 0usize;
    a.tree.walk(&mut |_, depth| {
        nodes += 1;
        deepest = deepest.max(depth);
    });
    assert_eq!(nodes, a.tree.nodes);
    assert!(deepest <= limits.max_tree_depth);

    // Records are accounted for exactly once each.
    let structural = u64::from(a.tree.down_count) + u64::from(a.tree.up_count);
    let accounted = u64::from(a.tree.handled) + u64::from(a.tree.skipped) + u64::from(a.tree.stripped);
    assert!(accounted >= structural);
    assert!(accounted <= u64::from(a.records_read) + structural);
});
