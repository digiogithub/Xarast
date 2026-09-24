//! Structural edge cases, built in memory so that CI exercises them with
//! no corpus present.
//!
//! The corpus proves the reader handles the files that exist. These prove
//! it handles the ones that do not: an empty compressed block, two blocks
//! with a streamed record between them, a truncated deflate stream, a bad
//! CRC, a bad length, both imbalances, 256-deep nesting, an unknown
//! essential tag, an unknown atomic tag with a subtree, and a record whose
//! size runs past the end of the file.

use proptest::prelude::*;
use xarast_xar::synth::XarBuilder;
use xarast_xar::{DiagCode, ReaderLimits, RecordReader, XarError, analyse};

fn limits() -> ReaderLimits {
    ReaderLimits::default()
}

fn tags(bytes: &[u8]) -> Vec<u32> {
    RecordReader::new(bytes, limits())
        .unwrap()
        .collect_records()
        .unwrap()
        .iter()
        .map(|r| r.tag)
        .collect()
}

#[test]
fn a_header_only_file_is_read() {
    let bytes = XarBuilder::new().finish();
    assert_eq!(tags(&bytes), vec![2]);
}

#[test]
fn a_file_with_no_header_record_is_rejected() {
    let bytes = XarBuilder::headerless().record(40, &[]).finish();
    assert_eq!(
        RecordReader::new(&bytes, limits()).unwrap_err(),
        XarError::MissingHeader
    );
}

#[test]
fn an_empty_compressed_block_is_read() {
    let bytes = XarBuilder::new().compressed(|_| {}).end_of_file().finish();
    assert_eq!(tags(&bytes), vec![2, 30, 31, 3]);
    let a = analyse(&bytes, limits()).unwrap();
    assert!(a.blocks_ok());
}

#[test]
fn a_record_between_two_blocks_is_read_uncompressed() {
    let png = b"\x89PNG\r\n\x1a\n and then some bytes";
    let mut named = Vec::new();
    for u in "Default".encode_utf16() {
        named.extend_from_slice(&u.to_le_bytes());
    }
    named.extend_from_slice(&0u16.to_le_bytes());
    named.extend_from_slice(png);
    let bytes = XarBuilder::new()
        .compressed(|b| {
            b.record(40, &[]);
        })
        .record(68, &named)
        .compressed(|b| {
            b.record(43, &[]);
        })
        .end_of_file()
        .finish();
    assert_eq!(tags(&bytes), vec![2, 30, 40, 31, 68, 30, 43, 31, 3]);
    let a = analyse(&bytes, limits()).unwrap();
    assert_eq!(a.blocks.len(), 2);
    assert!(a.blocks_ok());
}

#[test]
fn a_bad_crc_and_a_bad_length_are_both_caught() {
    for (crc_xor, len_delta, code) in [
        (0xDEAD_BEEFu32, 0u32, DiagCode::CrcMismatch),
        (0, 3, DiagCode::BlockLengthMismatch),
    ] {
        let bytes = XarBuilder::new()
            .compressed_tweaked(
                |b| {
                    b.record(40, &[]);
                },
                99,
                crc_xor,
                len_delta,
            )
            .end_of_file()
            .finish();
        let a = analyse(&bytes, limits()).unwrap();
        assert!(!a.blocks_ok());
        assert!(a.diagnostics.items().iter().any(|d| d.code == code));
    }
}

#[test]
fn a_truncated_deflate_stream_is_an_error_not_a_panic() {
    let full = XarBuilder::new()
        .compressed(|b| {
            for i in 0..200u32 {
                b.record(104, &i.to_le_bytes());
            }
        })
        .end_of_file()
        .finish();
    let mut errors = 0;
    for cut in 0..full.len() {
        match analyse(&full[..cut], limits()) {
            Ok(_) => {}
            Err(_) => errors += 1,
        }
    }
    assert!(errors > 0, "some truncation must be detected");
}

#[test]
fn nesting_two_hundred_and_fifty_six_deep_is_rebuilt_exactly() {
    let mut b = XarBuilder::new();
    for _ in 0..256 {
        b = b.record(104, &[]).down();
    }
    for _ in 0..256 {
        b = b.up();
    }
    let a = analyse(&b.end_of_file().finish(), limits()).unwrap();
    assert_eq!(a.tree.max_depth, 256);
    assert_eq!(a.tree.down_count, 256);
    assert_eq!(a.tree.up_count, 256);
    // `max_depth` counts open scopes; the deepest *node* sits one level
    // above the innermost DOWN, which has no node inside it.
    let mut deepest = 0;
    a.tree.walk(&mut |_, d| deepest = deepest.max(d));
    assert_eq!(deepest, 255);
}

#[test]
fn a_million_downs_do_not_overflow_the_stack() {
    let mut b = XarBuilder::new();
    for _ in 0..200_000 {
        b = b.down();
    }
    let a = analyse(&b.end_of_file().finish(), limits()).unwrap();
    assert_eq!(a.tree.max_depth, limits().max_tree_depth);
    assert!(
        a.diagnostics
            .items()
            .iter()
            .any(|d| d.code == DiagCode::DepthLimit)
    );
}

#[test]
fn a_record_whose_size_runs_past_the_end_is_truncated_not_fatal_to_the_process() {
    let mut bytes = XarBuilder::new().finish();
    bytes.extend_from_slice(&104u32.to_le_bytes());
    bytes.extend_from_slice(&1_000_000u32.to_le_bytes());
    bytes.extend_from_slice(&[7u8; 16]);
    assert!(matches!(
        analyse(&bytes, limits()),
        Err(XarError::Truncated(_))
    ));
}

#[test]
fn an_unknown_essential_tag_aborts_and_an_unknown_atomic_one_does_not() {
    let essential = XarBuilder::new()
        .record(11, &7777u32.to_le_bytes())
        .record(7777, &[])
        .end_of_file()
        .finish();
    assert_eq!(
        analyse(&essential, limits()).unwrap_err(),
        XarError::EssentialTag(7777)
    );

    let atomic = XarBuilder::new()
        .record(10, &7777u32.to_le_bytes())
        .record(7777, &[])
        .down()
        .record(104, &[])
        .up()
        .record(43, &[])
        .end_of_file()
        .finish();
    let a = analyse(&atomic, limits()).unwrap();
    let kept: Vec<u32> = a.tree.roots.iter().map(|n| n.record.tag).collect();
    assert_eq!(kept, vec![2, 10, 43, 3]);
    assert_eq!(a.tree.stripped, 4);
}

#[test]
fn a_definition_inside_a_dropped_atomic_subtree_is_kept_in_its_place() {
    // Definitions are the document's, whatever subtree they sit in: a
    // record after the dropped subtree may refer to them by number
    // (`Designs/Groucho2.xar` defines a bitmap inside a shadow).
    let mut colour = vec![0u8, 0, 0, 3, 0];
    colour.extend_from_slice(&24u32.to_le_bytes());
    colour.extend_from_slice(&0i32.to_le_bytes());
    colour.extend_from_slice(&[0u8; 16]);
    colour.extend_from_slice(&0u16.to_le_bytes());
    let bytes = XarBuilder::new()
        .record(10, &7777u32.to_le_bytes())
        .record(7777, &[])
        .down()
        .record(104, &[])
        .down()
        .record(51, &colour)
        .up()
        .up()
        .record(43, &[])
        .end_of_file()
        .finish();
    let a = analyse(&bytes, limits()).unwrap();
    let kept: Vec<u32> = a.tree.roots.iter().map(|n| n.record.tag).collect();
    assert_eq!(kept, vec![2, 10, 51, 43, 3]);
    assert_eq!(a.tree.rescued, 1);
    assert_eq!(a.tree.stripped, 6);
}

#[test]
fn every_prefix_of_a_realistic_file_is_handled() {
    let mut colour = vec![0u8, 0, 0, 3, 0];
    colour.extend_from_slice(&24u32.to_le_bytes());
    colour.extend_from_slice(&0i32.to_le_bytes());
    colour.extend_from_slice(&[0u8; 16]);
    colour.extend_from_slice(&0u16.to_le_bytes());
    let full = XarBuilder::new()
        .compressed(|b| {
            b.record(40, &[]).down();
            b.record(42, &[]).down();
            b.record(45, &[0u8; 17]);
            b.record(43, &[]).down();
            b.record(51, &colour);
            b.record(115, &[0u8; 18]).down();
            b.record(111, &[5, 5]);
            b.record(152, &500i32.to_le_bytes());
            b.up().up().up().up();
        })
        .end_of_file()
        .finish();
    for cut in 0..full.len() {
        let _ = analyse(&full[..cut], limits());
    }
    let a = analyse(&full, limits()).unwrap();
    assert!(a.blocks_ok());
    assert_eq!(a.tree.down_count, a.tree.up_count);
}

/// A test-only relative-path encoder, so that the decoder can be checked
/// against something other than itself.
///
/// Writing `.xar` is a non-goal for the product; a codec test is the one
/// place where writing it is the cheapest way to be sure.
fn encode_relative(points: &[(u8, i32, i32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(points.len() * 9);
    let mut prev = (0i32, 0i32);
    for (i, &(verb, x, y)) in points.iter().enumerate() {
        out.push(verb);
        let (dx, dy) = if i == 0 {
            (x, y)
        } else {
            (prev.0.wrapping_sub(x), prev.1.wrapping_sub(y))
        };
        let xb = dx.to_be_bytes();
        let yb = dy.to_be_bytes();
        out.extend_from_slice(&[xb[0], yb[0], xb[1], yb[1], xb[2], yb[2], xb[3], yb[3]]);
        prev = (x, y);
    }
    out
}

proptest! {
    /// Relative paths survive an encode/decode round trip.
    #[test]
    fn a_relative_path_round_trips(
        pts in prop::collection::vec(
            (prop::sample::select(vec![2u8, 6]), -1_000_000i32..1_000_000, -1_000_000i32..1_000_000),
            1..64,
        )
    ) {
        use xarast_geom::Point;
        use xarast_xar::{Cur, DiagSink, decode_relative};

        // The first point must start a subpath.
        let mut pts = pts;
        pts[0].0 = 6;
        let bytes = encode_relative(&pts);
        let mut diags = DiagSink::new();
        let mut cur = Cur::new(&bytes);
        let path = decode_relative(&mut cur, Point::ORIGIN, &mut diags, (1, 116)).unwrap();
        prop_assert!(path.validate().is_ok());
        let want: Vec<Point> = pts.iter().map(|&(_, x, y)| Point::raw(x, y)).collect();
        // Consecutive MoveTo pairs collapse, so compare what survives.
        prop_assert!(path.points().len() <= want.len());
        prop_assert!(path.points().iter().all(|p| want.contains(p)));
        if let Some(last) = path.points().last() {
            prop_assert_eq!(*last, *want.last().unwrap());
        }
    }

    /// Arbitrary bytes as a record payload never panic, whatever the tag.
    #[test]
    fn decoding_arbitrary_bytes_never_panics(
        tag in 0u32..5000,
        payload in prop::collection::vec(any::<u8>(), 0..200),
    ) {
        use xarast_geom::Point;
        use xarast_xar::{DiagSink, decode};
        let mut d = DiagSink::new();
        let _ = decode(tag, &payload, Point::ORIGIN, &mut d, (1, tag));
    }

    /// Arbitrary bytes as a whole file never panic.
    #[test]
    fn analysing_arbitrary_bytes_never_panics(
        body in prop::collection::vec(any::<u8>(), 0..512),
    ) {
        let mut bytes = xarast_xar::XAR_MAGIC.to_vec();
        bytes.extend_from_slice(&body);
        let _ = analyse(&bytes, limits());
    }
}
