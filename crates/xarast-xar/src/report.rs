//! Facts about a file, and the two ways `xar-dump` prints them.
//!
//! # The clean-room constraint this module exists to satisfy
//!
//! Dumping a `Designs/` file's coordinates, colour values and text and
//! committing that as a snapshot would amount to redistributing Xara's
//! artwork, which `docs/11-licensing-and-clean-room.md §3.2` forbids. So the
//! dump is split in two, and the split is enforced by types rather than by
//! review:
//!
//! * `--records`, `--tree` and `--model` print **content**. Interactive use
//!   only; never committed.
//! * `--stats`, `--tags` and `--corpus` print a [`FileReport`], which holds
//!   **only facts**: counts, histograms, depths, diagnostic codes. There is
//!   no field in it that can hold a coordinate, a colour component or a
//!   string taken from the file. That output *is* committed, and a test
//!   greps it for anything resembling file content.
//!
//! The one string a [`FileReport`] carries is the label the caller passes
//! in — a path the caller already knows, which `tests/corpus/corpus.lock`
//! records anyway.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::decode::{Decoded, decode};
use crate::diag::{DiagSink, Severity};
use crate::error::XarError;
use crate::reader::ReaderLimits;
use crate::tree::{FileAnalysis, RecordNode, analyse};
use xarast_geom::Point;

/// Everything `xar-dump --stats` knows, and nothing it may not print.
#[derive(Clone, Debug, Default)]
pub struct FileReport {
    /// What the caller called this file.
    pub label: String,
    /// Its size in bytes.
    pub size_bytes: u64,
    /// `CXN`, `CXW` or `CXM`, or empty when the header did not parse.
    pub file_type: &'static str,
    /// Whether the producer strings were present. Their *contents* are not
    /// recorded: that is file content.
    pub has_producer: bool,
    /// Records handed out by the physical layer.
    pub records: u32,
    /// How many distinct tags occurred.
    pub distinct_tags: usize,
    /// `TAG_DOWN` count.
    pub down: u32,
    /// `TAG_UP` count.
    pub up: u32,
    /// The deepest `DOWN` nesting.
    pub max_depth: usize,
    /// Nodes in the rebuilt tree.
    pub nodes: usize,
    /// Records a decoder understands.
    pub handled: u32,
    /// Records skipped for want of a decoder.
    pub skipped: u32,
    /// Records dropped with an atomic subtree.
    pub stripped: u32,
    /// Compressed blocks found.
    pub blocks: usize,
    /// Compressed blocks whose CRC and length both verified.
    pub blocks_ok: usize,
    /// Bytes after `TAG_ENDOFFILE`.
    pub trailing_bytes: u64,
    /// Records that decoded without error.
    pub decoded: u32,
    /// Records whose payload was too short for their layout.
    pub decode_errors: u32,
    /// Colour definitions registered.
    pub colours: usize,
    /// Path records decoded.
    pub path_records: u32,
    /// Points across all decoded paths.
    pub path_points: u64,
    /// Bitmap definitions found.
    pub bitmaps: usize,
    /// Bitmap definitions whose embedded bytes match their declared
    /// format's magic.
    pub bitmaps_magic_ok: usize,
    /// UTF-16 code units across all `TAG_TEXT_STRING` records.
    pub text_code_units: u64,
    /// Occurrences per tag.
    pub histogram: BTreeMap<u32, u32>,
    /// Occurrences per diagnostic code.
    pub diagnostics: BTreeMap<&'static str, u64>,
    /// Informational, warning and error diagnostic counts.
    pub severities: [u64; 3],
    /// Tags with no decoder whose class is structural.
    pub unknown_structural: Vec<u32>,
    /// The atomic tag list the file declared.
    pub atomic_tags: Vec<u32>,
    /// Why the file could not be read, if it could not. The message is the
    /// error's own text, which is built from constants and numbers.
    pub error: Option<String>,
}

impl FileReport {
    /// Whether the file was read end to end with no [`XarError`].
    #[must_use]
    pub const fn ok(&self) -> bool {
        self.error.is_none()
    }

    /// Whether `DOWN` and `UP` balanced.
    #[must_use]
    pub const fn balanced(&self) -> bool {
        self.down == self.up
    }

    /// How many error-severity diagnostics were raised.
    #[must_use]
    pub const fn errors(&self) -> u64 {
        self.severities[2]
    }

    /// How many warning-severity diagnostics were raised.
    #[must_use]
    pub const fn warnings(&self) -> u64 {
        self.severities[1]
    }

    /// A human-readable summary. Facts only.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "file           {}", self.label);
        let _ = writeln!(s, "size           {} bytes", self.size_bytes);
        let _ = writeln!(s, "type           {}", self.file_type);
        if let Some(e) = &self.error {
            let _ = writeln!(s, "error          {e}");
        }
        let _ = writeln!(s, "records        {}", self.records);
        let _ = writeln!(s, "distinct tags  {}", self.distinct_tags);
        let _ = writeln!(
            s,
            "down/up        {}/{} ({})",
            self.down,
            self.up,
            if self.balanced() {
                "balanced"
            } else {
                "unbalanced"
            }
        );
        let _ = writeln!(s, "max depth      {}", self.max_depth);
        let _ = writeln!(s, "nodes          {}", self.nodes);
        let _ = writeln!(
            s,
            "handled        {} handled, {} skipped, {} stripped",
            self.handled, self.skipped, self.stripped
        );
        let _ = writeln!(
            s,
            "blocks         {} of {} verified",
            self.blocks_ok, self.blocks
        );
        let _ = writeln!(s, "trailing bytes {}", self.trailing_bytes);
        let _ = writeln!(
            s,
            "decoded        {} ok, {} short",
            self.decoded, self.decode_errors
        );
        let _ = writeln!(
            s,
            "content        {} colours, {} paths ({} points), {} bitmaps ({} magic ok), {} text units",
            self.colours,
            self.path_records,
            self.path_points,
            self.bitmaps,
            self.bitmaps_magic_ok,
            self.text_code_units
        );
        let _ = writeln!(
            s,
            "diagnostics    {} info, {} warning, {} error",
            self.severities[0], self.severities[1], self.severities[2]
        );
        for (code, n) in &self.diagnostics {
            let _ = writeln!(s, "  {code} x{n}");
        }
        if !self.unknown_structural.is_empty() {
            let _ = writeln!(s, "unknown structural tags {:?}", self.unknown_structural);
        }
        s
    }

    /// The tag histogram, most frequent first. Facts only.
    #[must_use]
    pub fn tag_table(&self) -> String {
        let mut rows: Vec<(u32, u32)> = self.histogram.iter().map(|(t, n)| (*t, *n)).collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut s = String::new();
        let _ = writeln!(s, "{:>6}  {:>9}  {:<10} name", "tag", "count", "class");
        for (tag, n) in rows {
            let name = crate::tags::name_of(tag).unwrap_or("(undefined)");
            let class = crate::tags::class_of(tag).map_or("-", crate::TagClass::as_str);
            let mark = if crate::has_decoder(tag) { ' ' } else { '!' };
            let _ = writeln!(s, "{tag:>6}  {n:>9}  {class:<10} {mark}{name}");
        }
        s
    }

    /// A machine-readable summary. Facts only.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push('{');
        let _ = write!(s, "\"file\":\"{}\"", escape(&self.label));
        let _ = write!(s, ",\"size\":{}", self.size_bytes);
        let _ = write!(s, ",\"type\":\"{}\"", self.file_type);
        let _ = write!(s, ",\"ok\":{}", self.ok());
        if let Some(e) = &self.error {
            let _ = write!(s, ",\"error\":\"{}\"", escape(e));
        }
        let _ = write!(s, ",\"records\":{}", self.records);
        let _ = write!(s, ",\"distinctTags\":{}", self.distinct_tags);
        let _ = write!(s, ",\"down\":{}", self.down);
        let _ = write!(s, ",\"up\":{}", self.up);
        let _ = write!(s, ",\"maxDepth\":{}", self.max_depth);
        let _ = write!(s, ",\"nodes\":{}", self.nodes);
        let _ = write!(s, ",\"handled\":{}", self.handled);
        let _ = write!(s, ",\"skipped\":{}", self.skipped);
        let _ = write!(s, ",\"stripped\":{}", self.stripped);
        let _ = write!(s, ",\"blocks\":{}", self.blocks);
        let _ = write!(s, ",\"blocksOk\":{}", self.blocks_ok);
        let _ = write!(s, ",\"trailingBytes\":{}", self.trailing_bytes);
        let _ = write!(s, ",\"decoded\":{}", self.decoded);
        let _ = write!(s, ",\"decodeErrors\":{}", self.decode_errors);
        let _ = write!(s, ",\"colours\":{}", self.colours);
        let _ = write!(s, ",\"pathRecords\":{}", self.path_records);
        let _ = write!(s, ",\"pathPoints\":{}", self.path_points);
        let _ = write!(s, ",\"bitmaps\":{}", self.bitmaps);
        let _ = write!(s, ",\"bitmapsMagicOk\":{}", self.bitmaps_magic_ok);
        let _ = write!(s, ",\"textCodeUnits\":{}", self.text_code_units);
        let _ = write!(
            s,
            ",\"severities\":{{\"info\":{},\"warning\":{},\"error\":{}}}",
            self.severities[0], self.severities[1], self.severities[2]
        );
        s.push_str(",\"diagnostics\":{");
        for (i, (code, n)) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "\"{code}\":{n}");
        }
        s.push_str("},\"histogram\":{");
        for (i, (tag, n)) in self.histogram.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "\"{tag}\":{n}");
        }
        s.push_str("},\"unknownStructural\":[");
        for (i, t) in self.unknown_structural.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{t}");
        }
        s.push_str("],\"atomicTags\":[");
        for (i, t) in self.atomic_tags.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{t}");
        }
        s.push_str("]}");
        s
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Reads a file and reports the facts about it.
///
/// Never fails: a file that cannot be read produces a report whose
/// [`FileReport::error`] says why, which is what a batch run over a corpus
/// needs.
#[must_use]
pub fn xar_dump_report(label: &str, bytes: &[u8], limits: ReaderLimits) -> FileReport {
    let mut r = FileReport {
        label: label.to_owned(),
        size_bytes: bytes.len() as u64,
        ..FileReport::default()
    };
    match analyse(bytes, limits) {
        Ok(a) => fill(&mut r, &a),
        Err(e) => {
            r.error = Some(e.to_string());
            // A header-only probe still tells us the type.
            if let Ok(h) = crate::probe(bytes) {
                r.file_type = h.file_type.as_str();
                r.has_producer = h.producer.is_some();
            }
        }
    }
    r
}

fn fill(r: &mut FileReport, a: &FileAnalysis) {
    r.file_type = a.header.file_type.as_str();
    r.has_producer = a.header.producer.is_some();
    r.records = a.records_read;
    r.distinct_tags = a.tree.distinct_tags();
    r.down = a.tree.down_count;
    r.up = a.tree.up_count;
    r.max_depth = a.tree.max_depth;
    r.nodes = a.tree.nodes;
    r.handled = a.tree.handled;
    r.skipped = a.tree.skipped;
    r.stripped = a.tree.stripped;
    r.blocks = a.blocks.len();
    r.blocks_ok = a.blocks.iter().filter(|b| b.ok).count();
    r.trailing_bytes = a.trailing_bytes;
    r.histogram = a.tree.histogram.clone();
    r.unknown_structural = a.unknown_structural_tags();
    r.atomic_tags = a.policy.atomic().collect();

    let mut pass = DecodePass::default();
    let mut diags = DiagSink::new();
    a.tree.walk(&mut |node: &RecordNode, _depth| {
        pass.visit(node, &mut diags);
    });
    r.decoded = pass.decoded;
    r.decode_errors = pass.errors;
    r.colours = pass.colours.len();
    r.path_records = pass.path_records;
    r.path_points = pass.path_points;
    r.bitmaps = pass.bitmaps;
    r.bitmaps_magic_ok = pass.bitmaps_magic_ok;
    r.text_code_units = pass.text_code_units;

    let mut all = a.diagnostics.clone();
    all.absorb(&diags);
    for s in [Severity::Info, Severity::Warning, Severity::Error] {
        let slot = match s {
            Severity::Info => 0usize,
            Severity::Warning => 1,
            Severity::Error => 2,
        };
        if let Some(c) = r.severities.get_mut(slot) {
            *c = all.count(s);
        }
    }
    for d in all.items() {
        let slot = r.diagnostics.entry(d.code.as_str()).or_insert(0);
        *slot = slot.saturating_add(1);
    }
}

/// Runs every record through [`decode`], counting what came out.
///
/// The coordinate origin is tracked here because the point/vector
/// distinction is a decoding concern: `TAG_SPREADINFORMATION` is where the
/// original sets it, and [`crate::import::spread_origin`] is where we
/// derive it. It is the pasteboard margin, which is non-zero in 57 of the
/// 59 corpus files; nothing in this report depends on its value, but a
/// decode pass that used the wrong one would be a lie about what the file
/// says.
#[derive(Default)]
struct DecodePass {
    origin: Point,
    decoded: u32,
    errors: u32,
    colours: crate::ColourRegistry,
    path_records: u32,
    path_points: u64,
    bitmaps: usize,
    bitmaps_magic_ok: usize,
    text_code_units: u64,
}

impl DecodePass {
    fn visit(&mut self, node: &RecordNode, diags: &mut DiagSink) {
        let rec = &node.record;
        let at = (rec.number, rec.tag);
        match decode(rec.tag, &rec.data, self.origin, diags, at) {
            Ok(d) => {
                self.decoded = self.decoded.saturating_add(1);
                self.absorb(&d, rec, diags);
            }
            Err(XarError::ShortRecord(_)) => {
                self.errors = self.errors.saturating_add(1);
                diags.push(
                    crate::Diagnostic::new(crate::DiagCode::TruncatedRecord)
                        .at(rec.number, rec.tag)
                        .with_detail(rec.data.len() as u64),
                );
            }
            Err(_) => self.errors = self.errors.saturating_add(1),
        }
    }

    fn absorb(&mut self, d: &Decoded, rec: &crate::Record, diags: &mut DiagSink) {
        match d {
            Decoded::Path { path, .. } => {
                self.path_records = self.path_records.saturating_add(1);
                self.path_points = self.path_points.saturating_add(path.points().len() as u64);
            }
            Decoded::ColourDefinition(c) => {
                self.colours.define(rec.number, c, diags);
            }
            Decoded::BitmapDefinition(b) => {
                self.bitmaps = self.bitmaps.saturating_add(1);
                let bytes = rec.data.get(b.image.clone()).unwrap_or(&[]);
                if b.format.matches_magic(bytes) {
                    self.bitmaps_magic_ok = self.bitmaps_magic_ok.saturating_add(1);
                }
            }
            Decoded::TextString(s) => {
                self.text_code_units = self
                    .text_code_units
                    .saturating_add(s.encode_utf16().count() as u64);
            }
            Decoded::SpreadInformation(info) => {
                if info.width.raw() > 0 && info.height.raw() > 0 {
                    self.origin = crate::import::spread_origin(info);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::XarBuilder;

    #[test]
    fn a_report_of_a_synthetic_file_has_the_right_counts() {
        let bytes = XarBuilder::new()
            .compressed(|b| {
                b.record(40, &[]).down().record(104, &[]).up();
            })
            .end_of_file()
            .finish();
        let r = xar_dump_report("synthetic", &bytes, ReaderLimits::default());
        assert!(r.ok());
        assert_eq!(r.file_type, "CXN");
        assert_eq!(r.records, 8);
        assert_eq!(r.down, 1);
        assert_eq!(r.up, 1);
        assert!(r.balanced());
        assert_eq!(r.blocks, 1);
        assert_eq!(r.blocks_ok, 1);
        assert_eq!(r.trailing_bytes, 0);
        assert!(r.unknown_structural.is_empty());
    }

    #[test]
    fn a_broken_file_reports_rather_than_failing() {
        let r = xar_dump_report("broken", b"nonsense", ReaderLimits::default());
        assert!(!r.ok());
        assert!(r.error.is_some());
    }

    #[test]
    fn the_json_and_text_forms_carry_no_file_content() {
        let mut payload = Vec::new();
        for u in "SECRET ARTWORK".encode_utf16() {
            payload.extend_from_slice(&u.to_le_bytes());
        }
        let bytes = XarBuilder::new()
            .record(2201, &payload)
            .record(48, &{
                let mut v = vec![1u8];
                for u in "SECRET LAYER".encode_utf16() {
                    v.extend_from_slice(&u.to_le_bytes());
                }
                v.extend_from_slice(&0u16.to_le_bytes());
                v
            })
            .end_of_file()
            .finish();
        let r = xar_dump_report("leak-check", &bytes, ReaderLimits::default());
        for out in [r.to_text(), r.to_json(), r.tag_table()] {
            assert!(!out.contains("SECRET"), "content leaked into a fact dump");
        }
        assert_eq!(r.text_code_units, 14);
    }

    #[test]
    fn the_tag_table_marks_tags_with_no_decoder() {
        let bytes = XarBuilder::new()
            .record(3506, &[0u8; 45])
            .end_of_file()
            .finish();
        let r = xar_dump_report("unhandled", &bytes, ReaderLimits::default());
        assert!(r.tag_table().contains("!TAG_PRINTERSETTINGS"));
        assert_eq!(r.skipped, 1);
    }
}
