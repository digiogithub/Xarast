//! The physical layer against arbitrary bytes.
//!
//! Invariants, in the order they are likely to break:
//!
//! 1. **Never panic.** Not on a truncated deflate stream, not on a record
//!    that claims four gigabytes, not on a `TAG_ENDCOMPRESSION` with no
//!    matching start.
//! 2. **Terminate.** Every record consumes at least its eight header bytes
//!    from the stream, so the loop is structurally finite. Asserted below
//!    rather than assumed.
//! 3. **Bounded allocation.** The reader is given limits scaled to the
//!    input, so a small input cannot cost a large amount of memory
//!    whatever it claims about itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_xar::{ReaderLimits, RecordReader};

fuzz_target!(|data: &[u8]| {
    // Tight limits: the fuzzer runs millions of cases, and the invariant is
    // about behaviour, not about how much a real file may cost.
    let limits = ReaderLimits {
        max_inflated_per_block: 1 << 20,
        inflation_ratio: 100,
        max_total_inflated: 8 << 20,
        max_record_size: 1 << 20,
        max_records: 200_000,
        max_tree_depth: 256,
    };
    let Ok(mut reader) = RecordReader::new(data, limits) else {
        return;
    };
    let mut seen = 0u32;
    let mut last_number = 0u32;
    while let Some(rec) = reader.next_record() {
        let Ok(rec) = rec else { break };
        // Record numbering is the format's only pointer: it must be dense
        // and increasing, or every reference in the file resolves wrongly.
        assert_eq!(rec.number, last_number + 1, "record numbering is not dense");
        last_number = rec.number;
        seen += 1;
        assert!(seen <= 200_001, "the record loop did not terminate");
    }
    // Every block report is self-consistent.
    for b in reader.blocks() {
        assert_eq!(
            b.ok,
            b.crc_file == b.crc_computed && b.length_file == b.length_computed
        );
    }
});
