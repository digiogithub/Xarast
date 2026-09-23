//! Regression tests for findings of the `cargo fuzz` targets in `fuzz/`.
//!
//! Every input here is synthetic: either a minimised fuzzer artefact that
//! no longer resembles any real file, or a hand-built equivalent of one.
//! The fuzzer runs nightly; these run on every push.

use xarast_xar::synth::XarBuilder;
use xarast_xar::{ImportOptions, ReaderLimits, import};

fn opts() -> ImportOptions {
    ImportOptions {
        reader: ReaderLimits {
            max_inflated_per_block: 1 << 20,
            inflation_ratio: 100,
            max_total_inflated: 8 << 20,
            max_record_size: 1 << 20,
            max_records: 50_000,
            max_tree_depth: 64,
        },
        build: xarast_doc::BuildLimits::small(),
        strict: false,
        skip_text: false,
        skip_bitmaps: false,
    }
}

/// Imports and asserts the two whole-file properties `fuzz_xar_import`
/// checks: "valid or nothing", and the record accounting balances.
fn assert_import_invariants(bytes: &[u8]) {
    let Ok((doc, report)) = import(bytes, &opts()) else {
        return;
    };
    let check = doc.validate();
    assert!(check.errors.is_empty(), "{:?}", check.errors.first());
    assert_eq!(
        report.records_mapped + report.records_skipped + report.records_stripped,
        report.records_read,
        "every record is mapped, skipped or stripped exactly once"
    );
}

/// `fuzz_xar_import`, first run: a `TAG_NODE_BITMAP` (198) whose bitmap
/// reference resolves to nothing becomes an opaque node, and was counted as
/// mapped twice — once as a bitmap node, once as an opaque one.
#[test]
fn an_unresolved_node_bitmap_is_counted_once() {
    let bytes = XarBuilder::new()
        .record(198, &[0u8; 36])
        .end_of_file()
        .finish();
    assert_import_invariants(&bytes);
}
