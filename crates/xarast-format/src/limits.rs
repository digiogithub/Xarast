//! Hard limits against zip bombs and hostile XML (`research/06 §10.5`).
//!
//! Every limit is checked **before** the allocation it guards
//! (`research/06 §13.4` item 6): declared sizes are compared against these
//! numbers first, and reads are then capped at the declared size so that a
//! lying header cannot make a reader allocate more than it promised.

/// Reader limits. `Default` gives the normative values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum decompression ratio per entry (uncompressed ÷ compressed).
    /// Normative default 200:1.
    pub max_entry_ratio: u64,
    /// Entries whose uncompressed size is at most this many bytes are exempt
    /// from the ratio check. A ratio is meaningless on tiny inputs (64 KiB of
    /// spaces deflates to ~100 bytes, 650:1), and a bomb that stays under
    /// this size is harmless by construction. Default 1 MiB.
    pub ratio_floor: u64,
    /// Maximum total uncompressed size of all entries. Default 4 GiB.
    pub max_total_uncompressed: u64,
    /// Maximum number of entries. The classic end record cannot count past
    /// 65,535; beyond that the archive must be ZIP64, and the normative cap
    /// for ZIP64 is 1,000,000.
    pub max_entries: u64,
    /// Maximum XML nesting depth. Default 256.
    pub max_xml_depth: u32,
    /// Maximum uncompressed size of `META-INF/manifest.xml`. The manifest is
    /// parsed eagerly on open, so it gets a tighter cap than other entries.
    /// Default 16 MiB (a 1,000,000-entry manifest is ~250 MiB, so a package
    /// that large needs the caller to raise this deliberately).
    pub max_manifest_size: u64,
}

impl Limits {
    /// The normative limits of `research/06 §10.5`.
    pub const DEFAULT: Limits = Limits {
        max_entry_ratio: 200,
        ratio_floor: 1 << 20,
        max_total_uncompressed: 4 << 30,
        max_entries: 1_000_000,
        max_xml_depth: 256,
        max_manifest_size: 16 << 20,
    };

    /// Tight limits for fuzzing: nothing a 1 MiB input can declare gets past
    /// them into a large allocation.
    pub const FUZZ: Limits = Limits {
        max_entry_ratio: 200,
        ratio_floor: 64 << 10,
        max_total_uncompressed: 64 << 20,
        max_entries: 4096,
        max_xml_depth: 64,
        max_manifest_size: 1 << 20,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
