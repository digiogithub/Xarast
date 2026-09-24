//! Phase 10 acceptance criterion 13 (W10.6): brightness + contrast +
//! gamma on a 24 Mpx image reads back as the fused lookup table says, and
//! the table does not depend on anything but the operations.

use std::time::Instant;

use xarast_image::photo::{PointOp, Recipe, evaluate, fuse};

#[test]
fn a_24_mpx_chain_matches_the_fused_table() {
    let (w, h) = (6000u32, 4000u32);
    let master: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = i.wrapping_mul(2_654_435_761);
            [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8]
        })
        .collect();
    let ops = [
        PointOp::Gamma(1.25),
        PointOp::Brightness(0.08),
        PointOp::Contrast(0.15),
    ];
    let lut = fuse(&ops);
    let recipe = Recipe {
        lut: Some(lut.clone()),
        ..Recipe::default()
    };
    let t = Instant::now();
    let (ow, oh, out) = evaluate(w, h, &master, &recipe).unwrap();
    let took = t.elapsed();
    eprintln!("24 Mpx fused LUT: {took:?}");
    assert_eq!((ow, oh), (w, h));
    for (i, (src, dst)) in master
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks::<4>().0)
        .enumerate()
    {
        let want = [
            lut.0[0][usize::from(src[0])],
            lut.0[1][usize::from(src[1])],
            lut.0[2][usize::from(src[2])],
            src[3],
        ];
        assert_eq!(*dst, want, "pixel {i}");
    }
    // The table is the chain rounded once, and a second evaluation is the
    // same bytes (the pixel budget re-creates evicted images this way).
    let again = evaluate(w, h, &master, &recipe).unwrap().2;
    assert!(again == out);
}
