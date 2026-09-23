//! The manifest parser against arbitrary text.
//!
//! Invariants:
//!
//! 1. **Never panic**: not on a DOCTYPE, an undefined entity, an unbound
//!    prefix, 10,000 levels of nesting or invalid UTF-8.
//! 2. **Write–parse is a fixed point after one round.** Whatever parses and
//!    can be written parses again to the same model, and writing that model
//!    gives the same bytes. Foreign fragments are made namespace-complete on
//!    the first read, so the fixed point is reached on the first re-save,
//!    which is what "open and save twice gives the same file" needs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_format::Limits;
use xarast_format::manifest::Manifest;

fuzz_target!(|data: &[u8]| {
    let Ok(m) = Manifest::parse(data, &Limits::FUZZ) else {
        return;
    };
    // Writing can refuse (a foreign attribute name that is not an NCName,
    // say); that is an error, not a finding.
    let Ok(xml1) = m.to_xml() else {
        return;
    };
    let m1 = Manifest::parse(xml1.as_bytes(), &Limits::DEFAULT)
        .unwrap_or_else(|e| panic!("our own manifest does not parse: {e}\n{xml1}"));
    assert_eq!(m1.entries.len(), m.entries.len());
    // The meaning of the capabilities survives. Their foreign data may gain
    // namespace declarations on the first round (that is what makes it a
    // fixed point afterwards), and prefixes are only hints.
    let meaning = |m: &Manifest| -> Vec<(String, bool)> {
        m.requires
            .iter()
            .map(|c| (c.name.clone(), c.optional))
            .collect()
    };
    assert_eq!(meaning(&m1), meaning(&m));
    assert_eq!(m1.version, m.version);
    let xml2 = m1.to_xml().expect("a parsed manifest of ours writes");
    let m2 = Manifest::parse(xml2.as_bytes(), &Limits::DEFAULT).expect("second parse");
    assert_eq!(m1, m2, "not a fixed point after one round");
    assert_eq!(xml2, m2.to_xml().expect("third write"));
});
