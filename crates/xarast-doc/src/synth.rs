//! Synthetic documents, for benchmarks and for tests that need scale.
//!
//! The shape is taken from the corpus statistics in `research/01 §12.2`: one
//! spread, six layers, groups nested to depth six, an average of eight
//! children per group and about 40 % attribute nodes. It is not a pretty
//! picture; it is a realistic *shape*, which is what a traversal benchmark
//! measures.

use std::sync::Arc;

use xarast_geom::{Mp, Path, Point, Vector};

use crate::Document;
use crate::attr::AttrValue;
use crate::builder::{BuildLimits, DocumentBuilder};
use crate::fill::Paint;
use crate::kind::{NodeKind, PathNode, ShapeKind, ShapeNode};
use crate::structure::{LayerNode, PageNode, SpreadNode};

/// How the generated document is shaped.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SynthSpec {
    /// Roughly how many nodes to emit.
    pub nodes: usize,
    /// How many layers to spread them over.
    pub layers: usize,
    /// How deep groups nest.
    pub depth: usize,
    /// How many children a group has.
    pub fan_out: usize,
    /// What fraction of the nodes are attributes, in percent.
    pub attr_percent: u32,
}

impl Default for SynthSpec {
    fn default() -> SynthSpec {
        SynthSpec {
            nodes: 100_000,
            layers: 6,
            depth: 6,
            fan_out: 8,
            attr_percent: 40,
        }
    }
}

/// A deterministic generator, so that two runs of a benchmark measure the same
/// document.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// Builds a synthetic document of roughly `spec.nodes` nodes.
///
/// # Panics
///
/// Never in practice: the build script it emits is balanced and within the
/// default limits. A panic here means the builder itself is broken, which is
/// what the benchmark would otherwise silently measure.
#[must_use]
pub fn synthetic_document(spec: SynthSpec) -> Document {
    let mut b = DocumentBuilder::new(BuildLimits::default());
    let mut rng = Lcg(0x5EED_1234_ABCD_0001);
    let mut emitted = 1usize;

    b.node(NodeKind::Chapter).expect("chapter");
    b.push_scope().expect("chapter scope");
    let spread = SpreadNode::default();
    let rect = spread.page_size;
    b.node(NodeKind::Spread(Box::new(spread))).expect("spread");
    b.push_scope().expect("spread scope");
    b.node(NodeKind::Page(Box::new(PageNode {
        rect,
        right_hand: false,
    })))
    .expect("page");
    b.node(NodeKind::Grid(Box::default())).expect("grid");
    emitted += 4;

    let per_layer = spec.nodes / spec.layers.max(1);
    for i in 0..spec.layers {
        let mut layer = LayerNode::named(&format!("Layer {}", i + 1));
        layer.active = i == 0;
        b.node(NodeKind::Layer(Box::new(layer))).expect("layer");
        b.push_scope().expect("layer scope");
        emitted += 1;
        let mut in_layer = 0usize;
        while in_layer < per_layer && emitted < spec.nodes {
            let n = subtree(&mut b, &mut rng, spec, 0, spec.nodes - emitted);
            in_layer += n;
            emitted += n;
        }
        b.pop_scope();
    }

    let (doc, _) = b.finish().expect("a synthetic document always finishes");
    doc
}

fn subtree(
    b: &mut DocumentBuilder,
    rng: &mut Lcg,
    spec: SynthSpec,
    depth: usize,
    budget: usize,
) -> usize {
    if budget == 0 {
        return 0;
    }
    if depth >= spec.depth {
        return leaf(b, rng);
    }
    b.node(NodeKind::Group(Box::default())).expect("group");
    let mut count = 1usize;
    b.push_scope().expect("group scope");
    for _ in 0..spec.fan_out {
        if count >= budget {
            break;
        }
        if rng.below(100) < u64::from(spec.attr_percent) {
            count += attribute(b, rng);
        } else {
            count += subtree(b, rng, spec, depth + 1, budget - count);
        }
    }
    b.pop_scope();
    count
}

fn attribute(b: &mut DocumentBuilder, rng: &mut Lcg) -> usize {
    let v = match rng.below(4) {
        0 => AttrValue::LineWidth(Mp::new(rng.below(4_000) as i32)),
        1 => AttrValue::Fill(Paint::Flat {
            value: xarast_color::Colour::Direct(xarast_color::ColourValue::rgbt(
                rng.below(100) as f32 / 100.0,
                0.5,
                0.25,
                0.0,
            )),
        }),
        2 => AttrValue::JoinType(xarast_geom::Join::Round),
        _ => AttrValue::WindingRule(xarast_geom::FillRule::EvenOdd),
    };
    b.attribute(v).expect("attribute");
    1
}

fn leaf(b: &mut DocumentBuilder, rng: &mut Lcg) -> usize {
    let x = rng.below(500_000) as i32;
    let y = rng.below(700_000) as i32;
    if rng.below(2) == 0 {
        let mut p = Path::builder();
        p.rect(xarast_geom::Rect::new(
            Point::raw(x, y),
            Point::raw(x + 10_000, y + 8_000),
        ));
        b.node(NodeKind::Path(Box::new(PathNode {
            data: Arc::new(p.build()),
            filled: true,
            stroked: true,
        })))
        .expect("path");
    } else {
        b.node(NodeKind::Shape(Box::new(ShapeNode {
            shape: ShapeKind::Rect,
            origin: Point::raw(x, y),
            major: Vector::raw(9_000, 0),
            minor: Vector::raw(0, 6_000),
        })))
        .expect("shape");
    }
    1
}
