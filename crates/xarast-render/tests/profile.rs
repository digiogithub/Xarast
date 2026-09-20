//! The bias/gain profile against the formulas in `research/03 §2.6.3`.
//!
//! The renderer does not define its own profile type: it reuses
//! `xarast-geom`'s `BiasGain`, which is the same Schlick bias and gain with
//! one difference — the epsilon that keeps Schlick's parameter off its
//! poles is 1e-6 there and 1e-5 in the research document. This test
//! measures that difference rather than asserting it away, so that the
//! number is on the record.

use xarast_render::Profile;

/// The research document's formulas, transcribed exactly.
fn research_profile(bias: f64, gain: f64, x: f64) -> f64 {
    const EPS: f64 = 1e-5;
    let b = (bias + 1.0) * (0.5 - EPS) + EPS;
    let g = (gain + 1.0) * (0.5 - EPS) + EPS;
    let biased = x * b / ((1.0 - 2.0 * b) * (1.0 - x) + b);
    let c = (1.0 - 2.0 * g) * (1.0 - 2.0 * biased);
    if biased < 0.5 {
        biased * g / (c + g)
    } else {
        (c - biased * g) / (c - g)
    }
}

#[test]
fn a_thousand_tabulated_triples_agree_with_the_research_formula() {
    let mut worst = 0f64;
    let mut worst_at = (0.0, 0.0, 0.0);
    for bi in 0..10 {
        for gi in 0..10 {
            let bias = -0.9 + f64::from(bi) * 0.2;
            let gain = -0.9 + f64::from(gi) * 0.2;
            let p = Profile::new(bias, gain);
            for xi in 0..10 {
                let x = f64::from(xi) / 9.0;
                let d = (p.map(x) - research_profile(bias, gain, x)).abs();
                if d > worst {
                    worst = d;
                    worst_at = (bias, gain, x);
                }
            }
        }
    }
    // The only difference is the epsilon: 1e-6 against 1e-5, which moves
    // the curve by a few times 1e-5 -- a hundredth of an 8-bit step, and
    // therefore invisible in any 256- or 2048-entry table.
    assert!(
        worst < 1e-4,
        "worst deviation {worst} at bias/gain/x {worst_at:?}"
    );
    assert!(
        worst > 0.0,
        "if this becomes exact, xarast-geom has adopted the research epsilon; \
         update docs/memory/render.md"
    );
    eprintln!("worst profile deviation from research/03 2.6.3: {worst:e} at {worst_at:?}");
}

#[test]
fn the_identity_is_exact_and_short_circuited() {
    let p = Profile::IDENTITY;
    for i in 0..=1000 {
        let x = f64::from(i) / 1000.0;
        assert_eq!(p.map(x), x, "the identity profile must be exact at {x}");
    }
}

#[test]
fn the_profile_is_monotone_and_fixes_both_ends() {
    for bi in -5..=5 {
        for gi in -5..=5 {
            let p = Profile::new(f64::from(bi) / 5.0, f64::from(gi) / 5.0);
            assert!(p.map(0.0).abs() < 1e-9, "0 is a fixed point");
            assert!((p.map(1.0) - 1.0).abs() < 1e-9, "1 is a fixed point");
            let mut prev = p.map(0.0);
            for i in 1..=200 {
                let v = p.map(f64::from(i) / 200.0);
                assert!(
                    v >= prev - 1e-12,
                    "not monotone at bias {bi} gain {gi}: {prev} then {v}"
                );
                prev = v;
            }
        }
    }
}

#[test]
fn out_of_range_inputs_are_clamped_rather_than_producing_nan() {
    let p = Profile::new(0.5, -0.5);
    assert!(p.map(f64::NAN).is_finite());
    assert!(p.map(-1.0).is_finite());
    assert!(p.map(2.0).is_finite());
}
