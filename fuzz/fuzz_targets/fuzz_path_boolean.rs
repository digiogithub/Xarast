//! Two arbitrary paths through a boolean operation and fill rule.
//!
//! The property: whatever the operands — self-intersecting, coincident,
//! degenerate, open, spanning the whole document extent — `boolean`
//! returns, and what it returns is a well-formed `Path`. `self_union`,
//! which offsetting relies on, is held to the same standard.
//!
//! The fuzzer picks the operation and the rule rather than the target
//! running all sixteen pairs per input: an extent-sized curve flattens to
//! thousands of vertices at `Tolerance::BOOLEAN`, and sixteen overlays of
//! that per case spend the time budget on repetition instead of on new
//! inputs. Every pair is still reached, one input at a time.

#![no_main]

mod common;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use xarast_geom::{BoolOp, FillRule, Path, Tolerance, boolean, self_union};

#[derive(Arbitrary, Debug)]
struct Input {
    op: u8,
    rule: u8,
    a: Vec<common::PathOp>,
    b: Vec<common::PathOp>,
}

const OPS: [BoolOp; 4] = [
    BoolOp::Union,
    BoolOp::Intersection,
    BoolOp::Difference,
    BoolOp::Xor,
];

const RULES: [FillRule; 4] = [
    FillRule::NonZero,
    FillRule::EvenOdd,
    FillRule::Positive,
    FillRule::Negative,
];

fuzz_target!(|input: Input| {
    let a = common::build_path(&input.a);
    let b = common::build_path(&input.b);
    a.validate().expect("the builder produced a valid operand");
    b.validate().expect("the builder produced a valid operand");

    let op = OPS[usize::from(input.op) % OPS.len()];
    let rule = RULES[usize::from(input.rule) % RULES.len()];
    let r = boolean(&a, &b, op, rule, Tolerance::BOOLEAN);
    if let Err(e) = r.validate() {
        panic!("{op:?} under {rule:?} produced an invalid path: {e:?}");
    }
    let u = self_union(&a, rule, Tolerance::BOOLEAN);
    if let Err(e) = u.validate() {
        panic!("self_union under {rule:?} produced an invalid path: {e:?}");
    }

    // Identities that hold exactly on the integer engine.
    let empty = Path::new();
    assert!(
        boolean(&a, &empty, BoolOp::Intersection, FillRule::NonZero, Tolerance::BOOLEAN)
            .is_empty(),
        "A intersected with nothing must be empty"
    );
    assert!(
        boolean(&a, &a, BoolOp::Difference, FillRule::NonZero, Tolerance::BOOLEAN).is_empty(),
        "A minus A must be empty"
    );
});
