//! Type detection from the first 64 bytes (`research/06 §9.2`).
//!
//! `PK\x03\x04` at 0, `mimetype` at 30 and `application/vnd.xarast+zip` at
//! 38. Nothing is decompressed. This works only because the writer emits
//! `mimetype` first, STORED, with no extra field — which is what
//! `crate::writer` guarantees and `tests/container.rs` asserts.

#![deny(clippy::arithmetic_side_effects)]

use std::io::{self, Read, Seek, SeekFrom};

use crate::MIME_TYPE;

/// Bytes a sniffer needs.
pub const SIGNATURE_LEN: usize = 64;

/// `true` if `head` starts with the `.xarast` signature. Fewer than 64 bytes
/// is never a match.
pub fn sniff_bytes(head: &[u8]) -> bool {
    let Some(head) = head.get(..SIGNATURE_LEN) else {
        return false;
    };
    let field = |at: usize, len: usize| head.get(at..at.saturating_add(len));
    field(0, 4) == Some(b"PK\x03\x04")
        // General-purpose flags: no encryption (bit 0) and no data
        // descriptor (bit 3), or the sizes at 18/22 are not trustworthy.
        && field(6, 2).is_some_and(|f| f.first().is_some_and(|b| b & 0b1001 == 0))
        // Method 0, STORED.
        && field(8, 2) == Some(&[0, 0])
        // Compressed and uncompressed size both 26.
        && field(18, 4) == Some(&[26, 0, 0, 0])
        && field(22, 4) == Some(&[26, 0, 0, 0])
        // Name length 8, extra field length 0.
        && field(26, 4) == Some(&[8, 0, 0, 0])
        && field(30, 8) == Some(b"mimetype")
        && field(38, 26) == Some(MIME_TYPE.as_bytes())
}

/// Reads the first 64 bytes of `r` and checks the signature. The stream
/// position is restored afterwards, so the same reader can be handed on to
/// [`crate::XarastReader::open`].
pub fn sniff<R: Read + Seek + ?Sized>(r: &mut R) -> io::Result<bool> {
    let pos = r.stream_position()?;
    r.seek(SeekFrom::Start(0))?;
    let mut head = [0u8; SIGNATURE_LEN];
    let mut filled = 0usize;
    // `read_exact` would turn a short file into an error; a short file is
    // simply not a match.
    while let Some(rest) = head.get_mut(filled..) {
        if rest.is_empty() {
            break;
        }
        match r.read(rest) {
            Ok(0) => break,
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    r.seek(SeekFrom::Start(pos))?;
    Ok(sniff_bytes(head.get(..filled).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Vec<u8> {
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(b"PK\x03\x04");
        h[4] = 20;
        h[18] = 26;
        h[22] = 26;
        h[26] = 8;
        h[30..38].copy_from_slice(b"mimetype");
        h[38..64].copy_from_slice(MIME_TYPE.as_bytes());
        h
    }

    #[test]
    fn accepts_the_signature() {
        assert!(sniff_bytes(&good()));
        let mut c = std::io::Cursor::new(good());
        c.set_position(5);
        assert!(sniff(&mut c).unwrap());
        assert_eq!(c.position(), 5);
    }

    #[test]
    fn rejects_each_deviation() {
        assert!(!sniff_bytes(&good()[..63]));
        for (at, v) in [
            (0, b'Q'),
            (6, 1),
            (6, 8),
            (8, 8),
            (18, 27),
            (22, 25),
            (28, 4),
            (30, b'M'),
            (63, b'Z'),
        ] {
            let mut h = good();
            h[at] = v;
            assert!(!sniff_bytes(&h), "byte {at}");
        }
        assert!(!sniff(&mut std::io::Cursor::new(b"PK\x03\x04")).unwrap());
    }
}
