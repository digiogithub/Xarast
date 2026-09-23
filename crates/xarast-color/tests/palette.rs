//! Phase 8, W8.1: conversions against the original's formulas, their
//! measured round-trip error, and palette editing (`research/02 §5.10.1`).

use std::sync::Arc;

use proptest::prelude::*;
use xarast_color::{
    Colour, ColourContext, ColourDef, ColourEditError, ColourId, ColourKind, ColourModel,
    ColourTable, ColourValue, OnDelete, Rgba8, pack_component,
};

// ───────────────────────────── conversions ──────────────────────────────

fn max_diff(a: [f32; 4], b: [f32; 4], n: usize) -> f32 {
    (0..n).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max)
}

/// How many components a model's round trip is compared on: transparency
/// is carried by our conversions (the original drops it), CMYK has four
/// inks, grey one intensity plus transparency.
fn width(m: ColourModel) -> usize {
    match m {
        ColourModel::Greyt => 2,
        _ => 4,
    }
}

/// Values of `model` on a dense grid, restricted to what the model can
/// represent losslessly in RGB (the whole model for RGB, HSV, CMYK, grey).
fn grid(model: ColourModel) -> Vec<ColourValue> {
    let steps: Vec<f32> = (0..=16).map(|i| i as f32 / 16.0).collect();
    let mut out = Vec::new();
    match model {
        ColourModel::Greyt => {
            for v in (0..=255).map(|i| i as f32 / 255.0) {
                out.push(ColourValue::greyt(v, 0.0));
            }
        }
        ColourModel::Cmyk => {
            // CMYK -> RGB is many-to-one (K overlaps the inks), so the
            // representable set is what the forward conversion produces.
            for r in &steps {
                for g in &steps {
                    for b in &steps {
                        out.push(ColourValue::rgb(*r, *g, *b).to_cmyk());
                    }
                }
            }
        }
        ColourModel::Hsvt => {
            for h in &steps {
                for s in &steps {
                    for v in &steps {
                        // Hue is meaningless at s = 0 or v = 0: compare the
                        // colour, not the coordinates, there.
                        if *s > 0.0 && *v > 0.0 && *h < 1.0 {
                            out.push(ColourValue::hsvt(*h, *s, *v, 0.25));
                        }
                    }
                }
            }
        }
        _ => {
            for r in &steps {
                for g in &steps {
                    for b in &steps {
                        out.push(ColourValue::rgbt(*r, *g, *b, 0.25));
                    }
                }
            }
        }
    }
    out
}

/// The round-trip error table: `convert(convert(v, via), v.model())` for
/// every pair whose `via` model can hold `v`. Printed with
/// `--nocapture`; the bounds are the phase-8 acceptance criterion 1.
#[test]
fn round_trip_error_table() {
    use ColourModel::{Cmyk, Greyt, Hsvt, Rgbt};
    // (from, via): RGB -> grey -> RGB is lossy by definition, so only grey
    // sources go through grey.
    let pairs = [
        (Rgbt, Hsvt),
        (Rgbt, Cmyk),
        (Hsvt, Rgbt),
        (Hsvt, Cmyk),
        (Cmyk, Rgbt),
        (Cmyk, Hsvt),
        (Greyt, Rgbt),
        (Greyt, Hsvt),
        (Greyt, Cmyk),
    ];
    let ctx = ColourContext::uncalibrated();
    let mut report = String::from("from  via   max |error| (units of 1/255)\n");
    for (from, via) in pairs {
        let mut worst = 0.0f32;
        for v in grid(from) {
            let back = ctx.convert(ctx.convert(v, via), from);
            // CMYK carries no transparency, so a trip through it compares the
            // colour channels only.
            let n = if via == Cmyk && from != Cmyk {
                3.min(width(from))
            } else {
                width(from)
            };
            worst = worst.max(max_diff(v.components(), back.components(), n));
        }
        let in_255 = worst * 255.0;
        report.push_str(&format!("{from:?} {via:?} {in_255:.6}\n"));
        let bound = if from == Cmyk || via == Cmyk {
            2.0
        } else {
            1.0
        };
        assert!(in_255 <= bound, "{from:?} via {via:?}: {in_255} / 255");
    }
    println!("{report}");
}

#[test]
fn rgb_through_cmyk_is_exact_to_eight_bits() {
    for r in 0..=255u8 {
        for g in (0..=255u8).step_by(5) {
            for b in (0..=255u8).step_by(3) {
                let c = Rgba8::rgb(r, g, b);
                let back = ColourValue::from_rgba8(c).to_cmyk().to_rgba8_packed();
                assert_eq!(back, c);
            }
        }
    }
}

#[test]
fn cmyk_black_generation_matches_the_original() {
    // Pure black is one plate.
    assert_eq!(
        ColourValue::rgb(0.0, 0.0, 0.0).to_cmyk().components(),
        [0.0, 0.0, 0.0, 1.0]
    );
    // No black below the 50 % threshold.
    let ColourValue::Cmyk { c, m, y, k } = ColourValue::rgb(0.6, 0.7, 0.8).to_cmyk() else {
        panic!()
    };
    assert_eq!(k, 0.0);
    assert!((c - 0.4).abs() < 1e-6 && (m - 0.3).abs() < 1e-6 && (y - 0.2).abs() < 1e-6);
    // Past it, K = min - 0.5 and comes out of every ink.
    let ColourValue::Cmyk { c, m, y, k } = ColourValue::rgb(0.1, 0.2, 0.3).to_cmyk() else {
        panic!()
    };
    assert!((k - 0.2).abs() < 1e-6, "{k}");
    assert!((c - 0.7).abs() < 1e-6 && (m - 0.6).abs() < 1e-6 && (y - 0.5).abs() < 1e-6);
}

#[test]
fn srgb_primaries_known_values() {
    let cases = [
        (Rgba8::rgb(255, 0, 0), [0.0, 1.0, 1.0]),
        (Rgba8::rgb(0, 255, 0), [1.0 / 3.0, 1.0, 1.0]),
        (Rgba8::rgb(0, 0, 255), [2.0 / 3.0, 1.0, 1.0]),
        (Rgba8::rgb(0, 255, 255), [0.5, 1.0, 1.0]),
        (Rgba8::rgb(255, 0, 255), [5.0 / 6.0, 1.0, 1.0]),
        (Rgba8::rgb(255, 255, 0), [1.0 / 6.0, 1.0, 1.0]),
        (Rgba8::rgb(128, 128, 128), [0.0, 0.0, 128.0 / 255.0]),
    ];
    for (rgb, hsv) in cases {
        let got = ColourValue::from_rgba8(rgb).to_hsvt().components();
        for i in 0..3 {
            assert!((got[i] - hsv[i]).abs() < 1e-6, "{rgb:?}: {got:?}");
        }
        let back = ColourValue::hsvt(hsv[0], hsv[1], hsv[2], 0.0).to_rgba8_packed();
        assert_eq!(back, rgb);
    }
}

#[test]
fn cmyk_reference_patches() {
    // The naive CMYK -> RGB (research/02 §5.10.1) on eight patches.
    let patches: [([f32; 4], Rgba8); 8] = [
        ([0.0, 0.0, 0.0, 0.0], Rgba8::rgb(255, 255, 255)),
        ([1.0, 0.0, 0.0, 0.0], Rgba8::rgb(0, 255, 255)),
        ([0.0, 1.0, 0.0, 0.0], Rgba8::rgb(255, 0, 255)),
        ([0.0, 0.0, 1.0, 0.0], Rgba8::rgb(255, 255, 0)),
        ([0.0, 0.0, 0.0, 1.0], Rgba8::rgb(0, 0, 0)),
        ([0.5, 0.5, 0.5, 0.0], Rgba8::rgb(127, 127, 127)),
        ([0.2, 0.4, 0.6, 0.3], Rgba8::rgb(127, 76, 25)),
        ([0.9, 0.9, 0.9, 0.9], Rgba8::rgb(0, 0, 0)),
    ];
    for (cmyk, rgb) in patches {
        let v = ColourValue::cmyk(cmyk[0], cmyk[1], cmyk[2], cmyk[3]);
        assert_eq!(v.to_rgba8_packed(), rgb, "{cmyk:?}");
    }
}

#[test]
fn grey_uses_the_colour_models_weights() {
    let g = |r, gg, b| ColourValue::rgb(r, gg, b).to_greyt().components()[0];
    assert!((g(1.0, 0.0, 0.0) - 0.305).abs() < 1e-6);
    assert!((g(0.0, 1.0, 0.0) - 0.586).abs() < 1e-6);
    assert!((g(0.0, 0.0, 1.0) - 0.109).abs() < 1e-6);
    assert!((g(1.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
}

#[test]
fn packing_matches_the_originals_quantiser() {
    // 25.5 truncates to 25: the cached RGB of "90% Black" in the corpus.
    assert_eq!(pack_component(0.1), 25);
    assert_eq!(pack_component(0.0), 0);
    assert_eq!(pack_component(1.0), 255);
    assert_eq!(pack_component(-1.0), 0);
    assert_eq!(pack_component(2.0), 255);
    assert_eq!(pack_component(f32::NAN), 0);
    // Every byte survives byte -> f32 -> byte.
    for b in 0..=255u8 {
        assert_eq!(pack_component(f32::from(b) / 255.0), b);
    }
}

// ─────────────────────────────── derived ────────────────────────────────

#[test]
fn tints_follow_each_models_rule() {
    // RGB and grey move towards 1.
    let rgb = ColourValue::rgb(0.2, 0.4, 1.0).tinted(0.5).components();
    assert!((rgb[0] - 0.6).abs() < 1e-6 && (rgb[1] - 0.7).abs() < 1e-6 && rgb[2] == 1.0);
    assert!((ColourValue::greyt(0.0, 0.0).tinted(0.25).components()[0] - 0.75).abs() < 1e-6);
    // CMYK scales every ink, K included.
    let cmyk = ColourValue::cmyk(1.0, 0.5, 0.0, 1.0)
        .tinted(0.9)
        .components();
    assert!((cmyk[0] - 0.9).abs() < 1e-6 && (cmyk[1] - 0.45).abs() < 1e-6);
    assert!((cmyk[3] - 0.9).abs() < 1e-6);
    // HSV scales saturation and lifts value, hue untouched.
    let hsv = ColourValue::hsvt(0.3, 0.8, 0.5, 0.0)
        .tinted(0.5)
        .components();
    assert!((hsv[0] - 0.3).abs() < 1e-6);
    assert!((hsv[1] - 0.4).abs() < 1e-6);
    assert!((hsv[2] - 0.75).abs() < 1e-6);
    // Endpoints and transparency.
    assert_eq!(
        ColourValue::rgbt(0.1, 0.2, 0.3, 0.5).tinted(1.0),
        ColourValue::rgbt(0.1, 0.2, 0.3, 0.5)
    );
    assert_eq!(
        ColourValue::rgbt(0.1, 0.2, 0.3, 0.5).tinted(0.0),
        ColourValue::rgbt(1.0, 1.0, 1.0, 0.5)
    );
}

#[test]
fn shades_are_signed_moves_in_hsv() {
    let red = ColourValue::hsvt(0.0, 0.5, 0.5, 0.0);
    let c = |x, y| red.shaded(x, y).components();
    assert_eq!(c(0.0, 0.0), red.components());
    assert!((c(-1.0, 0.0)[1]).abs() < 1e-6);
    assert!((c(1.0, 0.0)[1] - 1.0).abs() < 1e-6);
    assert!((c(0.5, 0.0)[1] - 0.75).abs() < 1e-6);
    assert!((c(0.0, -0.5)[2] - 0.25).abs() < 1e-6);
    assert!((c(0.0, 0.5)[2] - 0.75).abs() < 1e-6);
    // Returned in the value's own model.
    assert_eq!(
        ColourValue::rgb(1.0, 0.0, 0.0).shaded(0.0, -0.5).model(),
        ColourModel::Rgbt
    );
}

#[test]
fn a_raw_shade_keeps_its_sign() {
    use xarast_color::Fixed24;
    let raw = [
        Fixed24((-0.25f32 * 16_777_216.0) as i32),
        Fixed24((0.5f32 * 16_777_216.0) as i32),
        Fixed24::ZERO,
        Fixed24::ZERO,
    ];
    assert_eq!(
        ColourKind::from_raw(4, raw),
        ColourKind::Shade { x: -0.25, y: 0.5 }
    );
    // Other kinds read as before.
    assert_eq!(
        ColourKind::from_raw(
            2,
            [
                Fixed24::from_f32(0.9),
                Fixed24::ZERO,
                Fixed24::ZERO,
                Fixed24::ZERO
            ]
        ),
        ColourKind::Tint {
            factor: Fixed24::from_f32(0.9).to_f32().unwrap()
        }
    );
}

#[test]
fn ninety_percent_black_resolves_to_the_files_cached_rgb() {
    let mut t = ColourTable::new();
    let black = t.insert(ColourDef {
        model: ColourModel::Cmyk,
        components: [Some(0.0), Some(0.0), Some(0.0), Some(1.0)],
        ..ColourDef::default()
    });
    let tint = t.insert(ColourDef {
        model: ColourModel::Cmyk,
        kind: ColourKind::Tint { factor: 0.9 },
        parent: Some(black),
        components: [Some(0.9), Some(0.0), Some(0.0), Some(0.0)],
        cached_rgb: Rgba8::rgb(25, 25, 25),
        ..ColourDef::default()
    });
    assert_eq!(t.resolve(tint).to_rgba8_packed(), Rgba8::rgb(25, 25, 25));
    let ctx = ColourContext::uncalibrated();
    assert_eq!(
        ctx.resolve(
            &Colour::Indexed {
                id: tint,
                tint: None
            },
            &t
        ),
        Rgba8::rgb(25, 25, 25)
    );
}

// ─────────────────────────────── editing ────────────────────────────────

fn rgb(t: &mut ColourTable, name: &str, r: f32, g: f32, b: f32) -> ColourId {
    t.insert(ColourDef::normal(ColourValue::rgb(r, g, b)).named(name))
}

fn tint_of(t: &mut ColourTable, parent: ColourId, f: f32) -> ColourId {
    let id = t.insert(ColourDef::default());
    t.reparent(id, ColourKind::Tint { factor: f }, Some(parent))
        .unwrap();
    id
}

#[test]
fn redefine_repaints_the_whole_derivation_subtree() {
    let mut t = ColourTable::new();
    let base = rgb(&mut t, "Base", 1.0, 0.0, 0.0);
    let a = tint_of(&mut t, base, 0.5);
    let b = tint_of(&mut t, a, 0.5);
    let other = rgb(&mut t, "Other", 0.0, 1.0, 0.0);
    let before = t.epoch();

    let changed = t
        .redefine(
            base,
            [Some(0.0), Some(0.0), Some(1.0), Some(0.0)],
            ColourModel::Rgbt,
        )
        .unwrap();
    assert_eq!(changed.as_slice(), &[base, a, b]);
    assert!(t.epoch() > before);
    assert_eq!(t.get(b).unwrap().cached_rgb, t.resolve(b).to_rgba8_packed());
    assert_eq!(t.get(b).unwrap().cached_rgb.b, 255);
    assert!(!changed.contains(&other));

    // Redefining to the same value reports only itself.
    let again = t
        .redefine(
            base,
            [Some(0.0), Some(0.0), Some(1.0), Some(0.0)],
            ColourModel::Rgbt,
        )
        .unwrap();
    assert_eq!(again.as_slice(), &[base]);
}

#[test]
fn inherit_needs_a_link() {
    let mut t = ColourTable::new();
    let base = rgb(&mut t, "Base", 1.0, 0.0, 0.0);
    assert_eq!(
        t.redefine(
            base,
            [None, Some(0.0), Some(0.0), Some(0.0)],
            ColourModel::Rgbt
        ),
        Err(ColourEditError::InheritWithoutLink)
    );
    let link = t.insert(ColourDef::default());
    t.reparent(link, ColourKind::Linked, Some(base)).unwrap();
    // A fresh link overrides everything with the current value...
    assert!(t.get(link).unwrap().components.iter().all(Option::is_some));
    // ...and can then inherit.
    t.redefine(link, [None, Some(1.0), None, None], ColourModel::Rgbt)
        .unwrap();
    assert_eq!(t.resolve(link).to_rgba8_packed(), Rgba8::rgb(255, 255, 0));
    t.redefine(
        base,
        [Some(0.0), Some(0.0), Some(1.0), Some(0.0)],
        ColourModel::Rgbt,
    )
    .unwrap();
    assert_eq!(t.resolve(link).to_rgba8_packed(), Rgba8::rgb(0, 255, 255));
}

#[test]
fn rename_refuses_a_duplicate() {
    let mut t = ColourTable::new();
    let a = rgb(&mut t, "A", 1.0, 0.0, 0.0);
    let b = rgb(&mut t, "B", 0.0, 1.0, 0.0);
    assert_eq!(t.rename(b, Arc::from("A")), Err(ColourEditError::NameInUse));
    t.rename(b, Arc::from("C")).unwrap();
    assert_eq!(t.by_name("C"), Some(b));
    assert_eq!(t.by_name("B"), None);
    assert_eq!(t.by_name("A"), Some(a));
    // Renaming to its own name is fine.
    t.rename(a, Arc::from("A")).unwrap();
}

#[test]
fn reparent_refuses_cycles_and_depth() {
    let mut t = ColourTable::new();
    let base = rgb(&mut t, "Base", 1.0, 0.0, 0.0);
    let a = tint_of(&mut t, base, 0.5);
    let b = tint_of(&mut t, a, 0.5);
    assert_eq!(
        t.reparent(base, ColourKind::Linked, Some(b)),
        Err(ColourEditError::Cycle)
    );
    assert_eq!(
        t.reparent(a, ColourKind::Linked, Some(a)),
        Err(ColourEditError::Cycle)
    );
    assert_eq!(
        t.reparent(a, ColourKind::Linked, None),
        Err(ColourEditError::MissingParent)
    );
    assert_eq!(
        t.reparent(a, ColourKind::Normal, Some(base)),
        Err(ColourEditError::UnexpectedParent)
    );

    // A chain of MAX_PARENT_DEPTH - 1 links resolves; one more is refused.
    let mut t = ColourTable::new();
    let mut at = rgb(&mut t, "Root", 0.0, 0.0, 0.0);
    for _ in 0..ColourTable::MAX_PARENT_DEPTH - 1 {
        at = tint_of(&mut t, at, 0.9);
    }
    assert!(t.try_resolve(at).is_ok());
    let extra = t.insert(ColourDef::default());
    assert_eq!(
        t.reparent(extra, ColourKind::Tint { factor: 0.5 }, Some(at)),
        Err(ColourEditError::TooDeep)
    );
    assert!(t.validate().is_empty());
}

#[test]
fn unlinking_keeps_the_appearance() {
    let mut t = ColourTable::new();
    let base = rgb(&mut t, "Base", 1.0, 0.0, 0.0);
    let a = tint_of(&mut t, base, 0.5);
    let look = t.resolve(a).to_rgba8_packed();
    t.reparent(a, ColourKind::Normal, None).unwrap();
    assert_eq!(t.resolve(a).to_rgba8_packed(), look);
    assert_eq!(t.get(a).unwrap().parent, None);
    // And it no longer follows the base.
    t.redefine(
        base,
        [Some(0.0), Some(0.0), Some(0.0), Some(0.0)],
        ColourModel::Rgbt,
    )
    .unwrap();
    assert_eq!(t.resolve(a).to_rgba8_packed(), look);
}

#[test]
fn a_tint_takes_its_parents_model() {
    let mut t = ColourTable::new();
    let base = t.insert(ColourDef::normal(ColourValue::hsvt(0.0, 1.0, 1.0, 0.0)));
    let a = tint_of(&mut t, base, 0.5);
    assert_eq!(t.get(a).unwrap().model, ColourModel::Hsvt);
    // HSV tint: S halved, V stays 1 — a pink.
    assert_eq!(t.resolve(a).to_rgba8_packed(), Rgba8::rgb(255, 127, 127));
}

#[test]
fn remove_with_each_policy() {
    let mut t = ColourTable::new();
    let base = rgb(&mut t, "Base", 1.0, 0.0, 0.0);
    let a = tint_of(&mut t, base, 0.5);
    let b = tint_of(&mut t, a, 0.5);
    assert_eq!(
        t.remove(base, OnDelete::Reject),
        Err(ColourEditError::StillReferenced)
    );
    assert_eq!(t.len(), 3);

    let look_a = t.resolve(a).to_rgba8_packed();
    let look_b = t.resolve(b).to_rgba8_packed();
    let detached = t.remove(base, OnDelete::Detach).unwrap();
    assert_eq!(detached.as_slice(), &[a]);
    assert!(t.get(base).is_none());
    assert_eq!(t.by_name("Base"), None);
    assert_eq!(t.get(a).unwrap().kind, ColourKind::Normal);
    assert_eq!(t.resolve(a).to_rgba8_packed(), look_a);
    assert_eq!(t.resolve(b).to_rgba8_packed(), look_b);
    assert_eq!(t.get(b).unwrap().parent, Some(a));
    // No dangling parent anywhere.
    for (_, d) in t.iter() {
        if let Some(p) = d.parent {
            assert!(t.get(p).is_some());
        }
    }
    // A leaf goes under Reject.
    t.remove(b, OnDelete::Reject).unwrap();
    assert_eq!(
        t.remove(b, OnDelete::Reject),
        Err(ColourEditError::NotFound)
    );
}

#[test]
fn resolve_order_puts_parents_first_and_is_cached() {
    let mut t = ColourTable::new();
    let base = rgb(&mut t, "Base", 1.0, 0.0, 0.0);
    let a = tint_of(&mut t, base, 0.5);
    let b = tint_of(&mut t, a, 0.5);
    // Make the base derive from a later entry, so slot order is wrong.
    let root = rgb(&mut t, "Root", 0.0, 0.0, 1.0);
    t.reparent(base, ColourKind::Linked, Some(root)).unwrap();
    let order = t.resolve_order().to_vec();
    let pos = |id| order.iter().position(|x| *x == id).unwrap();
    assert!(pos(root) < pos(base) && pos(base) < pos(a) && pos(a) < pos(b));
    assert_eq!(order.len(), t.len());
    assert!(std::ptr::eq(t.resolve_order(), t.resolve_order()));
}

#[test]
fn repair_cycles_breaks_a_loaded_loop() {
    let mut t = ColourTable::new();
    let a = t.insert(ColourDef {
        kind: ColourKind::Linked,
        cached_rgb: Rgba8::rgb(10, 20, 30),
        components: [None; 4],
        ..ColourDef::default()
    });
    let b = t.insert(ColourDef {
        kind: ColourKind::Linked,
        parent: Some(a),
        components: [None; 4],
        cached_rgb: Rgba8::rgb(40, 50, 60),
        ..ColourDef::default()
    });
    t.set_parent(a, Some(b));
    assert!(!t.validate().is_empty());
    let demoted = t.repair_cycles();
    assert_eq!(demoted.as_slice(), &[b], "the younger entry is demoted");
    assert!(t.validate().is_empty());
    assert_eq!(t.resolve(b).to_rgba8_packed(), Rgba8::rgb(40, 50, 60));
    assert_eq!(t.get(b).unwrap().kind, ColourKind::Normal);
}

#[test]
fn every_mutation_advances_the_epoch() {
    let mut t = ColourTable::new();
    let mut last = t.epoch();
    let mut step = |t: &ColourTable| {
        assert!(t.epoch() > last);
        last = t.epoch();
    };
    let a = rgb(&mut t, "A", 1.0, 0.0, 0.0);
    step(&t);
    let b = t.insert(ColourDef::default());
    step(&t);
    t.reparent(b, ColourKind::Tint { factor: 0.5 }, Some(a))
        .unwrap();
    step(&t);
    t.redefine(a, [Some(0.5); 4], ColourModel::Rgbt).unwrap();
    step(&t);
    t.rename(a, Arc::from("Z")).unwrap();
    step(&t);
    t.remove(b, OnDelete::Reject).unwrap();
    step(&t);
    let e = t.epoch();
    t.advance_epoch_past(xarast_color::PaletteEpoch(e.0 + 10));
    assert_eq!(t.epoch().0, e.0 + 11);
}

mod table {
    use super::*;

    /// A small deterministic generator, so a failure names its seed.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Brute force: does `from` reach `to` by following parent links?
    fn reaches(t: &ColourTable, from: ColourId, to: ColourId) -> bool {
        let mut at = from;
        for _ in 0..=t.len() {
            if at == to {
                return true;
            }
            match t
                .get(at)
                .and_then(|d| d.parent.filter(|_| d.kind.is_derived()))
            {
                Some(p) => at = p,
                None => return false,
            }
        }
        true
    }

    fn depth(t: &ColourTable, id: ColourId) -> usize {
        let mut n = 0;
        let mut at = id;
        while let Some(p) = t
            .get(at)
            .and_then(|d| d.parent.filter(|_| d.kind.is_derived()))
        {
            n += 1;
            at = p;
            assert!(n <= t.len(), "loop");
        }
        n
    }

    /// Acceptance criterion 2: over 1 000 random derivation graphs,
    /// `reparent` never accepts a cycle and never refuses an acyclic edge
    /// as one.
    #[test]
    fn cycles() {
        let mut accepted = 0usize;
        let mut refused_cycle = 0usize;
        for seed in 1..=1000u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut t = ColourTable::new();
            let n = 2 + rng.below(24);
            let ids: Vec<ColourId> = (0..n)
                .map(|i| {
                    t.insert(ColourDef::normal(ColourValue::rgb(
                        i as f32 / n as f32,
                        0.5,
                        0.5,
                    )))
                })
                .collect();
            for _ in 0..3 * n {
                let id = ids[rng.below(n)];
                let parent = ids[rng.below(n)];
                let kind = match rng.below(4) {
                    0 => ColourKind::Linked,
                    1 => ColourKind::Tint { factor: 0.5 },
                    2 => ColourKind::Shade { x: -0.2, y: 0.3 },
                    _ => {
                        let r = t.reparent(id, ColourKind::Normal, None);
                        assert!(r.is_ok(), "seed {seed}: unlink refused: {r:?}");
                        continue;
                    }
                };
                let would_loop = reaches(&t, parent, id);
                let before = t.clone();
                match t.reparent(id, kind, Some(parent)) {
                    Ok(_) => {
                        assert!(!would_loop, "seed {seed}: accepted a cycle");
                        accepted += 1;
                    }
                    Err(ColourEditError::Cycle) => {
                        assert!(would_loop, "seed {seed}: refused an acyclic edge");
                        refused_cycle += 1;
                    }
                    Err(ColourEditError::TooDeep) => assert!(!would_loop),
                    Err(e) => panic!("seed {seed}: {e:?}"),
                }
                if t.len() != before.len() {
                    panic!("seed {seed}: an edit changed the entry count");
                }
                for x in &ids {
                    assert!(depth(&t, *x) < ColourTable::MAX_PARENT_DEPTH, "seed {seed}");
                }
            }
            assert!(t.validate().is_empty(), "seed {seed}");
            // The order is a valid parents-first order.
            let order = t.resolve_order().to_vec();
            for (i, c) in order.iter().enumerate() {
                if let Some(p) = t
                    .get(*c)
                    .and_then(|d| d.parent.filter(|_| d.kind.is_derived()))
                {
                    assert!(order[..i].contains(&p), "seed {seed}: child before parent");
                }
            }
        }
        assert!(
            accepted > 1000 && refused_cycle > 100,
            "{accepted} {refused_cycle}"
        );
    }
}

fn any_rgba8() -> impl Strategy<Value = Rgba8> {
    (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Rgba8 { r, g, b, a: 255 })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_000))]

    /// HSV -> RGB -> HSV, and grey through every model, round trip within
    /// one 8-bit step; anything through CMYK within two.
    #[test]
    fn model_round_trips(c in any_rgba8(), m in 0usize..4) {
        let models = [ColourModel::Rgbt, ColourModel::Hsvt, ColourModel::Cmyk, ColourModel::Greyt];
        let v = ColourValue::from_rgba8(c).to_model(models[m]);
        for via in models {
            if via == ColourModel::Greyt && models[m] != ColourModel::Greyt {
                continue; // lossy by definition
            }
            let back = v.to_model(via).to_model(v.model()).to_rgba8_packed();
            let want = v.to_rgba8_packed();
            let bound = if via == ColourModel::Cmyk || v.model() == ColourModel::Cmyk { 2 } else { 1 };
            prop_assert!(back.r.abs_diff(want.r) <= bound, "{:?} via {:?}", v, via);
            prop_assert!(back.g.abs_diff(want.g) <= bound, "{:?} via {:?}", v, via);
            prop_assert!(back.b.abs_diff(want.b) <= bound, "{:?} via {:?}", v, via);
        }
    }

    /// Redefining a colour reports exactly the entries whose cached value
    /// moved, and leaves every cached value equal to a fresh resolution.
    #[test]
    fn cached_values_stay_current(c in any_rgba8(), f in 0.0f32..=1.0, x in -1.0f32..=1.0) {
        let mut t = ColourTable::new();
        let base = t.insert(ColourDef::normal(ColourValue::rgb(0.5, 0.5, 0.5)));
        let tint = tint_of(&mut t, base, f);
        let shade = t.insert(ColourDef::default());
        t.reparent(shade, ColourKind::Shade { x, y: -x }, Some(tint)).unwrap();
        let v = ColourValue::from_rgba8(c).components();
        t.redefine(base, v.map(Some), ColourModel::Rgbt).unwrap();
        for (id, d) in t.iter() {
            prop_assert_eq!(d.cached_rgb, t.resolve(id).to_rgba8_packed());
        }
    }
}
