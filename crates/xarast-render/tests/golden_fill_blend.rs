//! Golden level A for the phase-8 matrix (T8.5.5): every fill shape ×
//! every exposed blend family × {flat, graduated} transparency, rendered by
//! the CPU backend and compared **exactly** with committed images.
//!
//! The goldens live in `tests/golden/fill_blend/`, apart from the feature
//! corpus, because every other consumer of `corpus::all_cases` (export,
//! PDF, determinism, parity) would otherwise pay for 160 more scenes.
//!
//! Run with `XARAST_UPDATE_GOLDEN=1` to regenerate them from the current
//! code: a deliberate, reviewable act.

mod common;

use common::{artefact_dir, golden_dir, render_case, updating};
use xarast_render::Surface;
use xarast_render::corpus::{EXPOSED_FAMILIES, FILL_SHAPES, fill_blend_cases};
use xarast_render::golden::{Comparison, compare, diff_heatmap, read_png, write_png};

#[test]
fn the_fill_blend_matrix_matches_its_goldens_exactly() {
    let cases = fill_blend_cases();
    assert_eq!(cases.len(), 160, "8 shapes × 10 families × 2 variants");

    let dir = golden_dir().join("fill_blend");
    std::fs::create_dir_all(&dir).expect("creating the golden directory");
    let mut missing = Vec::new();
    let mut failures: Vec<(String, Comparison)> = Vec::new();

    for case in &cases {
        let rendered = render_case(case);
        let path = dir.join(format!("{}.png", case.name));
        if updating() || !path.exists() {
            write_png(&rendered, &path).expect("writing a golden");
            if !updating() {
                missing.push(case.name.clone());
            }
            continue;
        }
        let golden = read_png(&path).expect("reading a golden");
        let c = compare(&rendered, &golden);
        if !c.is_exact() {
            let art = artefact_dir().join("fill_blend");
            let _ = std::fs::create_dir_all(&art);
            let _ = write_png(&rendered, &art.join(format!("{}.actual.png", case.name)));
            let _ = write_png(&golden, &art.join(format!("{}.expected.png", case.name)));
            let _ = write_png(
                &diff_heatmap(&rendered, &golden),
                &art.join(format!("{}.diff.png", case.name)),
            );
            failures.push((case.name.clone(), c));
        }
    }

    assert!(
        missing.is_empty(),
        "{} goldens did not exist and have just been written; review and commit them: {:?}",
        missing.len(),
        &missing[..missing.len().min(8)]
    );
    assert!(
        failures.is_empty(),
        "{} of {} matrix scenes differ from their golden (artefacts in target/golden-diffs/fill_blend): {:?}",
        failures.len(),
        cases.len(),
        failures
            .iter()
            .take(8)
            .map(|(n, c)| format!(
                "{n}: {} px, max {}/255, dE {:.3}",
                c.differing_pixels, c.max_channel_delta, c.mean_delta_e
            ))
            .collect::<Vec<_>>()
    );
}

/// Phase-8 acceptance 14, per cell: for every shape and variant, the ten
/// families give ten different pictures (all 45 pairs differ). A family
/// that silently fell back to another would pass the golden gate once
/// blessed; it cannot pass this.
#[test]
fn every_exposed_family_draws_differently_for_every_shape_and_variant() {
    let cases = fill_blend_cases();
    let rendered: Vec<(String, Surface)> = cases
        .iter()
        .map(|c| (c.name.clone(), render_case(c)))
        .collect();
    let find = |name: &str| {
        rendered
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s)
            .unwrap_or_else(|| panic!("no case {name}"))
    };
    let mut same = Vec::new();
    for shape in FILL_SHAPES {
        for variant in ["flat", "graduated"] {
            let names: Vec<String> = cases
                .iter()
                .filter(|c| c.name.starts_with(&format!("{shape}_")) && c.name.ends_with(variant))
                .map(|c| c.name.clone())
                .collect();
            assert_eq!(names.len(), EXPOSED_FAMILIES.len(), "{shape} {variant}");
            for (i, a) in names.iter().enumerate() {
                for b in &names[i + 1..] {
                    if compare(find(a), find(b)).is_exact() {
                        same.push(format!("{a} == {b}"));
                    }
                }
            }
        }
    }
    assert!(same.is_empty(), "families that draw alike: {same:?}");
}
