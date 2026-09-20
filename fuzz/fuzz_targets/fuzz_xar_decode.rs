//! Every typed decoder, over whatever tree an arbitrary file produces.
//!
//! This is the target that stands in for `fuzz_xar_import` until the
//! document model lands: it exercises the same handlers and the same
//! reference resolution, and asserts the same "valid or nothing" property
//! at the level this crate stops at — a decoded path is always a path that
//! `xarast_geom` considers well formed.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_geom::Point;
use xarast_xar::{Decoded, DiagSink, ReaderLimits, analyse, decode};

fuzz_target!(|data: &[u8]| {
    let limits = ReaderLimits {
        max_inflated_per_block: 1 << 20,
        inflation_ratio: 100,
        max_total_inflated: 8 << 20,
        max_record_size: 1 << 20,
        max_records: 50_000,
        max_tree_depth: 64,
    };
    let Ok(a) = analyse(data, limits) else {
        return;
    };
    let mut diags = DiagSink::new();
    let mut colours = xarast_xar::ColourRegistry::new();
    a.tree.walk(&mut |node, _| {
        let r = &node.record;
        let at = (r.number, r.tag);
        let Ok(d) = decode(r.tag, &r.data, Point::ORIGIN, &mut diags, at) else {
            return;
        };
        match d {
            // Valid or nothing: a decoded path always passes validation.
            Decoded::Path { path, .. } => path.validate().expect("a decoded path is well formed"),
            Decoded::RegularShape(s) => {
                s.primary_edge.validate().expect("a shape edge is well formed");
                s.secondary_edge.validate().expect("a shape edge is well formed");
            }
            Decoded::ColourDefinition(c) => {
                let id = colours.define(r.number, &c, &mut diags);
                // Resolution terminates whatever the parent chain says.
                let _ = colours.table().resolve(id);
            }
            Decoded::BitmapDefinition(b) => {
                // The image range must be inside the payload it indexes.
                assert!(b.image.end <= r.data.len());
                assert!(b.image.start <= b.image.end);
            }
            _ => {}
        }
    });
});
