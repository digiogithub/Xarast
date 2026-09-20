//! Allocation must follow bytes actually read, never a declared length.
//!
//! This is the bug class that matters most in a legacy binary format: a
//! sixty-four byte file says one of its arrays has 4 294 967 295 entries,
//! the parser reserves for it, and the process dies before a single byte of
//! that array was ever going to arrive. Fuzzing finds it eventually; a
//! direct measurement finds it now, and says which field.
//!
//! Every length-bearing field in the format gets one case here. The
//! measurement is a counting global allocator, which is process-wide, so
//! **the tests in this file must not run at the same time**: one would
//! measure the other's allocations and the failure would look like a
//! regression in the parser. [`SERIAL`] is what stops that; it is not
//! optional and not a performance detail.

// A counting global allocator is the only way to measure peak allocation,
// and `GlobalAlloc` is an unsafe trait. Every method here forwards straight
// to `System`; the only additions are two atomics. Test-only.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use xarast_xar::synth::XarBuilder;
use xarast_xar::{ReaderLimits, analyse};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Counting;

// SAFETY: every method forwards to `System` with the same pointer and
// layout it was given, and does nothing else but update two atomics.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is forwarded unchanged to the system allocator,
        // which is what the caller's contract already permits.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            bump(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr` came from `System.alloc` with this same `layout`,
        // because every allocation in this binary goes through here.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `dealloc`, plus `new_size` is the caller's, forwarded
        // unchanged.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            bump(new_size);
        }
        p
    }
}

fn bump(n: usize) {
    let live = LIVE.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Serialises the tests in this file. See the module documentation.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs `f` and returns the peak live allocation during it, in bytes.
fn peak_of(f: impl FnOnce()) -> usize {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    f();
    PEAK.load(Ordering::Relaxed).saturating_sub(before)
}

const MIB: usize = 1 << 20;

/// A record whose payload declares `0xFFFF_FFFF`, or whose body declares an
/// absurd count, at the end of an otherwise tiny file.
fn tiny_file_with(tag: u32, payload: &[u8]) -> Vec<u8> {
    XarBuilder::new()
        .record(tag, payload)
        .end_of_file()
        .finish()
}

/// A record header that lies about its size, with no payload behind it.
fn tiny_file_with_lying_size(tag: u32) -> Vec<u8> {
    let mut v = XarBuilder::new().finish();
    v.extend_from_slice(&tag.to_le_bytes());
    v.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    v
}

#[test]
fn no_declared_length_can_drive_an_allocation() {
    let _serial = SERIAL.lock();
    let limits = ReaderLimits::default();
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();

    // 1. The record header's own `size` field.
    cases.push(("record size", tiny_file_with_lying_size(104)));

    // 2. A relative path's implied point count: `size / 9`, where the size
    //    is a lie.
    cases.push(("relative path point count", tiny_file_with_lying_size(116)));

    // 3. An absolute path's declared point count.
    let mut p = 0x7FFF_FFFFi32.to_le_bytes().to_vec();
    p.extend_from_slice(&[6, 0, 0, 0, 0, 0, 0, 0, 0]);
    cases.push(("absolute path point count", tiny_file_with(100, &p)));

    // 4. `TAG_ATOMICTAGS`, whose entry count is `size / 4`.
    cases.push(("atomic tag count", tiny_file_with_lying_size(10)));

    // 5. A multi-stage gradient's ramp stop count.
    let mut p = vec![0u8; 16];
    p.extend_from_slice(&0i32.to_le_bytes());
    p.extend_from_slice(&0i32.to_le_bytes());
    p.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    cases.push(("ramp stop count", tiny_file_with(4075, &p)));

    // 6. A dash pattern's element count.
    let mut p = vec![0u8; 8];
    p.extend_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
    cases.push(("dash element count", tiny_file_with(184, &p)));

    // 7. A string with no terminator, which must not scan forever.
    cases.push(("unterminated string", tiny_file_with(48, &[1u8; 48])));

    // 8. A bitmap payload size.
    cases.push(("bitmap payload size", tiny_file_with_lying_size(68)));

    // 9. `TAG_TAGDESCRIPTION`'s entry count.
    cases.push(("tag description count", tiny_file_with_lying_size(12)));

    for (what, bytes) in cases {
        assert!(bytes.len() < 256, "{what}: the input itself must be tiny");
        let peak = peak_of(|| {
            if let Ok(a) = analyse(&bytes, limits) {
                // Decode everything too: the counts inside payloads are
                // only reachable from there.
                let mut d = xarast_xar::DiagSink::new();
                a.tree.walk(&mut |n, _| {
                    let r = &n.record;
                    let _ = xarast_xar::decode(
                        r.tag,
                        &r.data,
                        xarast_geom::Point::ORIGIN,
                        &mut d,
                        (r.number, r.tag),
                    );
                });
            }
        });
        assert!(
            peak < MIB,
            "{what}: peak allocation {peak} bytes for a {}-byte input",
            bytes.len()
        );
    }
}

/// Criterion 20: a small input that claims to inflate to gigabytes is
/// refused, and refusing it stays cheap.
#[test]
fn a_decompression_bomb_is_refused_within_its_cap() {
    let _serial = SERIAL.lock();
    use std::io::Write as _;
    use xarast_xar::{RecordReader, XarError};

    // A kilobyte of deflate that expands a thousandfold. The bytes are
    // zeroes, which inflate into an endless run of empty records — the
    // worst case, because every one of them is individually valid.
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(&vec![0u8; 8 * MIB]).unwrap();
    let compressed = enc.finish().unwrap();
    assert!(
        compressed.len() < 16_384,
        "the bomb is small: {}",
        compressed.len()
    );

    let mut bytes = XarBuilder::new().record(30, &99u32.to_le_bytes()).finish();
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(&[0u8; 8]);

    // With the ratio tightened, the ratio itself is what stops it.
    let tight = ReaderLimits {
        max_inflated_per_block: 16 * 1024,
        inflation_ratio: 4,
        ..ReaderLimits::default()
    };
    let peak = peak_of(|| {
        let mut r = RecordReader::new(&bytes, tight).unwrap();
        let mut last = None;
        while let Some(rec) = r.next_record() {
            if let Err(e) = rec {
                last = Some(e);
                break;
            }
        }
        assert_eq!(last, Some(XarError::Limit("inflation ratio")));
    });
    assert!(peak < 128 * MIB, "peak allocation {peak} bytes");

    // With the defaults it is still refused, and still cheaply: the record
    // budget is bounded by what the input could hold.
    let peak = peak_of(|| {
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        let mut last = None;
        while let Some(rec) = r.next_record() {
            if let Err(e) = rec {
                last = Some(e);
                break;
            }
        }
        assert!(
            matches!(last, Some(XarError::Limit(_))),
            "a bomb must hit a limit, got {last:?}"
        );
    });
    assert!(peak < 128 * MIB, "peak allocation {peak} bytes");
}
