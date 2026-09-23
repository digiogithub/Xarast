//! A pre-flight check of the end-of-central-directory record.
//!
//! The `zip` crate sizes its first allocation from the entry count the end
//! record declares, and silently collapses duplicate names into one map slot.
//! Both have to be known *before* handing the file over: the count to enforce
//! [`Limits::max_entries`] before anything is allocated, and the count again
//! afterwards to detect duplicates (`research/06 §3.1`: a reader that finds
//! them MUST reject the file).
//!
//! The check is deliberately stricter than a general ZIP reader: the end
//! record must be the last thing in the file (a comment is allowed, trailing
//! garbage is not), the comment must not itself contain an end-record
//! signature (so the `zip` crate, which searches backwards, lands on the same
//! record), and the archive must be single-disk. A damaged file that fails
//! here is a job for `xarast repair` (F6.8), not for the open path.

#![deny(clippy::arithmetic_side_effects)]

use std::io::{self, Read, Seek, SeekFrom};

use crate::error::ReadError;
use crate::limits::Limits;

const EOCD_SIG: [u8; 4] = *b"PK\x05\x06";
const LOCATOR_SIG: [u8; 4] = *b"PK\x06\x07";
const EOCD64_SIG: [u8; 4] = *b"PK\x06\x06";
const EOCD_LEN: u64 = 22;
const LOCATOR_LEN: u64 = 20;
const EOCD64_MIN_LEN: u64 = 56;
/// A central directory header is at least 46 bytes.
const CDH_MIN_LEN: u64 = 46;
const MAX_COMMENT: u64 = 0xFFFF;

/// What the end record declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EndRecord {
    /// Total number of central directory entries.
    pub entries: u64,
    /// Whether the ZIP64 end record was present.
    pub zip64: bool,
}

fn corrupt(what: &'static str) -> ReadError {
    ReadError::Corrupt(what)
}

fn u16_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u64::from(u16::from_le_bytes(s.try_into().ok()?)))
}

fn u32_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u64::from(u32::from_le_bytes(s.try_into().ok()?)))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

fn read_at<R: Read + Seek + ?Sized>(r: &mut R, at: u64, buf: &mut [u8]) -> io::Result<()> {
    r.seek(SeekFrom::Start(at))?;
    r.read_exact(buf)
}

/// Reads and validates the end record. Leaves the stream position anywhere.
pub(crate) fn check<R: Read + Seek + ?Sized>(
    r: &mut R,
    limits: &Limits,
) -> Result<EndRecord, ReadError> {
    let file_len = r.seek(SeekFrom::End(0))?;
    if file_len < EOCD_LEN {
        return Err(corrupt("file too short for an end record"));
    }
    // The tail is at most 64 KiB + 22 bytes: bounded whatever the file says.
    let tail_len = file_len.min(EOCD_LEN.saturating_add(MAX_COMMENT));
    let tail_start = file_len.saturating_sub(tail_len);
    let mut tail = vec![0u8; usize::try_from(tail_len).map_err(|_| corrupt("tail"))?];
    read_at(r, tail_start, &mut tail)?;

    // Walk the signatures backwards and take the first whose comment ends
    // exactly at the end of the file. A signature inside that record's own
    // fixed fields is harmless (it cannot parse), but one inside its comment
    // could be a forged record that a backwards-searching reader would try
    // first, so it is refused.
    let consistent = |pos: usize| {
        let rec = tail.get(pos..)?;
        let comment_len = u16_at(rec, 20)?;
        (rec.len() as u64 == EOCD_LEN.saturating_add(comment_len)).then_some(rec)
    };
    let hits: Vec<usize> = tail
        .windows(4)
        .enumerate()
        .filter_map(|(i, w)| (w == EOCD_SIG).then_some(i))
        .collect();
    let (pos, rec) = hits
        .iter()
        .rev()
        .find_map(|&p| consistent(p).map(|rec| (p, rec)))
        .ok_or(corrupt(
            "no end-of-central-directory record at the end of the file",
        ))?;
    let comment_start = pos.saturating_add(EOCD_LEN as usize);
    if hits.iter().any(|&p| p >= comment_start) {
        return Err(corrupt("end-record signature inside the archive comment"));
    }
    let disk = u16_at(rec, 4).ok_or(corrupt("end record"))?;
    let cd_disk = u16_at(rec, 6).ok_or(corrupt("end record"))?;
    let on_disk = u16_at(rec, 8).ok_or(corrupt("end record"))?;
    let mut entries = u16_at(rec, 10).ok_or(corrupt("end record"))?;
    let mut cd_size = u32_at(rec, 12).ok_or(corrupt("end record"))?;
    let mut cd_offset = u32_at(rec, 16).ok_or(corrupt("end record"))?;
    if disk != 0 || cd_disk != 0 || on_disk != entries {
        return Err(corrupt("multi-disk archive"));
    }
    let eocd_at = tail_start.saturating_add(pos as u64);
    let mut dir_end = eocd_at;

    // ZIP64: a locator immediately before the end record.
    let mut zip64 = false;
    if let Some(loc_at) = eocd_at.checked_sub(LOCATOR_LEN) {
        let mut loc = [0u8; LOCATOR_LEN as usize];
        read_at(r, loc_at, &mut loc)?;
        if loc.get(..4) == Some(&LOCATOR_SIG[..]) {
            let loc_disk = u32_at(&loc, 4).ok_or(corrupt("zip64 locator"))?;
            let rec64_at = u64_at(&loc, 8).ok_or(corrupt("zip64 locator"))?;
            let disks = u32_at(&loc, 16).ok_or(corrupt("zip64 locator"))?;
            if loc_disk != 0 || disks > 1 {
                return Err(corrupt("multi-disk archive"));
            }
            if rec64_at
                .checked_add(EOCD64_MIN_LEN)
                .is_none_or(|end| end > loc_at)
            {
                return Err(corrupt("zip64 end record out of range"));
            }
            let mut rec64 = [0u8; EOCD64_MIN_LEN as usize];
            read_at(r, rec64_at, &mut rec64)?;
            if rec64.get(..4) != Some(&EOCD64_SIG[..]) {
                return Err(corrupt("zip64 end record signature"));
            }
            let d = u32_at(&rec64, 16).ok_or(corrupt("zip64 end record"))?;
            let cd = u32_at(&rec64, 20).ok_or(corrupt("zip64 end record"))?;
            let on = u64_at(&rec64, 24).ok_or(corrupt("zip64 end record"))?;
            let total = u64_at(&rec64, 32).ok_or(corrupt("zip64 end record"))?;
            if d != 0 || cd != 0 || on != total {
                return Err(corrupt("multi-disk archive"));
            }
            entries = total;
            cd_size = u64_at(&rec64, 40).ok_or(corrupt("zip64 end record"))?;
            cd_offset = u64_at(&rec64, 48).ok_or(corrupt("zip64 end record"))?;
            dir_end = rec64_at;
            zip64 = true;
        }
    }

    if entries > limits.max_entries {
        return Err(ReadError::TooManyEntries {
            declared: entries,
            limit: limits.max_entries,
        });
    }
    if cd_offset
        .checked_add(cd_size)
        .is_none_or(|end| end > dir_end)
    {
        return Err(corrupt("central directory out of range"));
    }
    if entries
        .checked_mul(CDH_MIN_LEN)
        .is_none_or(|min| min > cd_size)
    {
        return Err(corrupt("central directory too small for its entry count"));
    }
    Ok(EndRecord { entries, zip64 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn zip_with(names: &[&str]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for n in names {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"x").unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn counts_entries() {
        let z = zip_with(&["a", "b", "c"]);
        let e = check(&mut Cursor::new(&z), &Limits::DEFAULT).unwrap();
        assert_eq!(
            e,
            EndRecord {
                entries: 3,
                zip64: false
            }
        );
    }

    #[test]
    fn rejects_trailing_garbage_and_too_many_entries() {
        let mut z = zip_with(&["a"]);
        z.push(0);
        assert!(check(&mut Cursor::new(&z), &Limits::DEFAULT).is_err());

        let z = zip_with(&["a", "b"]);
        let lim = Limits {
            max_entries: 1,
            ..Limits::DEFAULT
        };
        assert!(matches!(
            check(&mut Cursor::new(&z), &lim),
            Err(ReadError::TooManyEntries {
                declared: 2,
                limit: 1
            })
        ));
    }

    #[test]
    fn rejects_an_inflated_count() {
        let mut z = zip_with(&["a"]);
        let n = z.len();
        // Total entries at EOCD+10, and on-disk at +8 to stay single-disk.
        z[n - 22 + 8] = 0xff;
        z[n - 22 + 10] = 0xff;
        assert!(check(&mut Cursor::new(&z), &Limits::DEFAULT).is_err());
    }

    #[test]
    fn rejects_a_signature_in_the_comment() {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        w.set_raw_comment(b"PK\x05\x06 not really".to_vec().into_boxed_slice())
            .unwrap();
        w.start_file("a", zip::write::SimpleFileOptions::default())
            .unwrap();
        let z = w.finish().unwrap().into_inner();
        assert!(check(&mut Cursor::new(&z), &Limits::DEFAULT).is_err());
    }

    #[test]
    fn short_and_empty_inputs() {
        for len in [0usize, 1, 21, 22, 100] {
            assert!(check(&mut Cursor::new(vec![0u8; len]), &Limits::DEFAULT).is_err());
        }
    }
}
