//! SVG path data, as arbitrary text.
//!
//! `.xarast` stores geometry as SVG path data, so this parser reads text a
//! user may have edited by hand or another program may have written. The
//! properties: parsing never panics; anything it accepts is a well-formed
//! `Path`; and a path this crate wrote reads back as exactly the same
//! path, which is the round trip the regression snapshots rely on.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_geom::Path;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(path) = Path::from_svg_path_data(text) else {
        return;
    };
    path.validate().expect("an accepted path is well formed");

    let written = path.to_svg_path_data();
    let reread = Path::from_svg_path_data(&written).expect("our own output parses");
    assert_eq!(reread, path, "the integer round trip is exact: {written}");
});
