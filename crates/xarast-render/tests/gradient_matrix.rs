//! Every gradient cell exists, is legal, and draws something.

mod common;

use common::render_case;
use xarast_render::corpus::all_cases;
use xarast_render::{ALL_MAPPINGS, ALL_REPEATS, ALL_SHAPES};

#[test]
fn every_cell_of_shape_by_repeat_by_mapping_is_covered() {
    let cases = all_cases();
    let gradients: Vec<_> = cases
        .iter()
        .filter(|c| c.name.starts_with("gradient_"))
        .collect();
    assert_eq!(
        gradients.len(),
        ALL_SHAPES.len() * ALL_REPEATS.len() * ALL_MAPPINGS.len(),
        "the gradient matrix is 6 x 4 x 2"
    );
}

#[test]
fn no_gradient_cell_renders_a_single_flat_colour() {
    // A cell that comes out flat is a cell whose maths silently did
    // nothing, which is exactly the failure a matrix test exists to catch.
    for case in all_cases().iter().filter(|c| c.name.starts_with("gradient_")) {
        let s = render_case(case);
        let mut distinct = std::collections::HashSet::new();
        for px in s.data().chunks_exact(4) {
            distinct.insert([px[0], px[1], px[2]]);
            if distinct.len() > 8 {
                break;
            }
        }
        assert!(
            distinct.len() > 8,
            "{} rendered only {} distinct colours",
            case.name,
            distinct.len()
        );
    }
}
