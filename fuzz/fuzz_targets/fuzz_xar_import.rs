//! The whole pipeline into `DocumentBuilder`, over arbitrary bytes.
//!
//! The property, stated so that it can be asserted rather than hoped for:
//! **valid or nothing**. Either [`import`] fails, or it returns a document
//! whose `validate()` has zero errors. There is no third outcome, and no
//! input — however hostile — may produce one.
//!
//! The limits are deliberately small. A fuzzer's job here is to find a
//! panic, an overflow or an unbounded allocation, not to import a
//! seven-megabyte drawing; the caps keep each case fast enough that the
//! run covers the handlers rather than the allocator.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_xar::{ImportOptions, ReaderLimits, import};

fuzz_target!(|data: &[u8]| {
    let opts = ImportOptions {
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
        bitmap_limits: xarast_image::DecodeLimits::tight(),
    };
    let Ok((doc, report)) = import(data, &opts) else {
        return;
    };
    let check = doc.validate();
    assert!(
        check.errors.is_empty(),
        "an imported document must have no validation errors: {:?}",
        check.errors.first()
    );
    // The accounting must balance whatever the input was: every record is
    // mapped, skipped or stripped, exactly once.
    assert_eq!(
        report.records_mapped + report.records_skipped + report.records_stripped,
        report.records_read,
    );
    // Bounded output: a document cannot hold more nodes than the build
    // limit allows, whatever the input claimed.
    assert!(report.nodes_built <= xarast_doc::BuildLimits::small().max_nodes);
});
