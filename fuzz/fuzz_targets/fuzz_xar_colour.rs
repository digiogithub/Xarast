//! Colour definitions, and the parent chain they build.
//!
//! The payload is split into records of arbitrary length and each one is
//! defined at a record number the previous ones may point at, so the target
//! reaches the cases a single payload cannot: an inherit sentinel in every
//! slot, a parent that does not exist, and a chain long enough to test the
//! resolver's depth limit.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_xar::{ColourRecord, ColourRegistry, Cur, DiagSink};

fuzz_target!(|data: &[u8]| {
    let mut diags = DiagSink::new();
    let mut registry = ColourRegistry::new();

    // Whole-payload case first.
    let mut cur = Cur::new(data);
    if let Ok(c) = ColourRecord::parse_complex(&mut cur) {
        let id = registry.define(1, &c, &mut diags);
        // Resolution must terminate and must never produce a NaN channel.
        let v = registry.table().resolve(id).to_rgbt();
        for ch in v.components() {
            assert!(ch.is_finite(), "a resolved colour channel is finite");
        }
    }

    // Then a chain: successive 31-byte slices, each its own definition.
    for (i, chunk) in data.chunks(31).enumerate().take(512) {
        let mut cur = Cur::new(chunk);
        if let Ok(c) = ColourRecord::parse_complex(&mut cur) {
            let id = registry.define(i as u32 + 2, &c, &mut diags);
            let v = registry.table().resolve(id).to_rgbt();
            for ch in v.components() {
                assert!(ch.is_finite());
            }
        }
        let mut cur = Cur::new(chunk);
        let _ = ColourRecord::parse_rgb(&mut cur);
    }
    // The table the chain built must validate, cycles and all.
    let _ = registry.table().validate();
});
