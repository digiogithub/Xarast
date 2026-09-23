//! Arbitrary build scripts through `DocumentBuilder`.
//!
//! The builder is the only way an importer creates a document, and it
//! promises exactly two outcomes: an error, or a document whose
//! `validate()` has no errors. This target holds it to that with inputs no
//! importer would produce on purpose but a corrupt file can — every node
//! kind at every level, unbalanced scopes, live objects without
//! controllers, text outside stories, layers without an active one, and
//! references to colours and bitmaps that were never defined.
//!
//! `BuildError::Inconsistent` is the builder admitting a repair failed —
//! "a bug in the builder, never in the input" — so it is a finding too.
//!
//! Fuzzing the builder directly is what lets `fuzz_xar_import` test the
//! parser rather than rediscover builder bugs.

#![no_main]

mod common;

use std::sync::Arc;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use xarast_color::{Colour, ColourDef, ColourId, ColourKind, ColourValue};
use xarast_doc::{
    AttrValue, BitmapId, BitmapNode, BuildError, BuildLimits, ClipViewNode, DocumentBuilder,
    EffectParams, FillGeometry, GridNode, GuidelineNode, LayerNode, LiveKind, LiveNode, LiveRole,
    NodeKind, OpaqueNode, PathNode, TextItem, TextLineNode, TextStoryNode,
};
use xarast_geom::{Cap, DashPattern, FillRule, Join, Vector};

const MAX_STEPS: usize = 256;

/// A resource reference: one that was defined earlier, or a forged key
/// that was never handed out.
#[derive(Arbitrary, Debug, Clone, Copy)]
enum Key {
    Defined(u8),
    Forged(u64),
}

#[derive(Arbitrary, Debug)]
enum Kind {
    Document,
    Chapter,
    Spread,
    Page,
    Layer { active: bool, guide: bool, guide_colour: Option<Key> },
    Grid,
    Path(Vec<common::PathOp>, bool, bool),
    Bitmap(Key, common::Pt),
    Guideline(bool, common::Coord, Option<Key>),
    Group,
    Live(u8, bool),
    ClipView,
    TextStory,
    TextLine,
    Char(char),
    Kern(common::Coord),
    Tab,
    LineBreak(bool),
    Opaque(u32, Vec<u8>),
}

#[derive(Arbitrary, Debug)]
enum Attr {
    LineWidth(common::Coord),
    Winding(u8),
    Join(u8),
    Cap(u8),
    MitreLimit(common::Coord),
    FontSize(common::Coord),
    AspectRatio(f32),
    Name(String),
    Clip(Vec<common::PathOp>),
    Dash(Vec<common::Coord>, common::Coord),
    FillIndexed(Key, Option<f32>),
    FillDirect(f32, f32, f32, f32),
}

#[derive(Arbitrary, Debug)]
enum Step {
    Down,
    Up,
    Node(Kind),
    Attr(Attr),
    Default(Attr),
    Colour {
        parent: Option<Key>,
        kind: u8,
        components: [Option<f32>; 4],
    },
}

struct Ctx {
    colours: Vec<ColourId>,
}

impl Ctx {
    fn colour(&self, k: Key) -> ColourId {
        match k {
            Key::Defined(i) if !self.colours.is_empty() => {
                self.colours[usize::from(i) % self.colours.len()]
            }
            Key::Defined(i) => slotmap::KeyData::from_ffi(u64::from(i)).into(),
            Key::Forged(v) => slotmap::KeyData::from_ffi(v).into(),
        }
    }

    fn bitmap(&self, k: Key) -> BitmapId {
        match k {
            Key::Defined(i) => slotmap::KeyData::from_ffi(u64::from(i)).into(),
            Key::Forged(v) => slotmap::KeyData::from_ffi(v).into(),
        }
    }

    fn node(&self, k: &Kind) -> NodeKind {
        match k {
            Kind::Document => NodeKind::Document(Box::default()),
            Kind::Chapter => NodeKind::Chapter,
            Kind::Spread => NodeKind::Spread(Box::default()),
            Kind::Page => NodeKind::Page(Box::default()),
            Kind::Layer {
                active,
                guide,
                guide_colour,
            } => NodeKind::Layer(Box::new(LayerNode {
                active: *active,
                guide: *guide,
                guide_colour: guide_colour.map(|c| self.colour(c)),
                ..LayerNode::default()
            })),
            Kind::Grid => NodeKind::Grid(Box::new(GridNode::default())),
            Kind::Path(ops, filled, stroked) => {
                let mut p = PathNode::new(common::build_path(ops));
                p.filled = *filled;
                p.stroked = *stroked;
                NodeKind::Path(Box::new(p))
            }
            Kind::Bitmap(k, origin) => NodeKind::Bitmap(Box::new(BitmapNode {
                image: self.bitmap(*k),
                origin: origin.point(),
                major: Vector::raw(1_000, 0),
                minor: Vector::raw(0, 1_000),
            })),
            Kind::Guideline(horizontal, position, colour) => {
                NodeKind::Guideline(Box::new(GuidelineNode {
                    horizontal: *horizontal,
                    position: position.mp(),
                    colour: colour.map(|c| self.colour(c)),
                }))
            }
            Kind::Group => NodeKind::Group(Box::default()),
            Kind::Live(role, locked) => NodeKind::Live(Box::new(LiveNode {
                role: match role % 3 {
                    0 => LiveRole::Controller,
                    1 => LiveRole::Source,
                    _ => LiveRole::Generated,
                },
                kind: LiveKind::Effect(Box::new(EffectParams {
                    id: Arc::from("fuzz"),
                    settings: Arc::from(&[][..]),
                    locked: *locked,
                })),
                regen: Default::default(),
                name: None,
            })),
            Kind::ClipView => NodeKind::ClipView(ClipViewNode::default()),
            Kind::TextStory => NodeKind::TextStory(Box::new(TextStoryNode::default())),
            Kind::TextLine => NodeKind::TextLine(Box::new(TextLineNode::default())),
            Kind::Char(c) => NodeKind::TextItem(TextItem::Char(*c)),
            Kind::Kern(k) => NodeKind::TextItem(TextItem::Kern(k.mp())),
            Kind::Tab => NodeKind::TextItem(TextItem::Tab),
            Kind::LineBreak(p) => NodeKind::TextItem(TextItem::LineBreak(*p)),
            Kind::Opaque(tag, payload) => NodeKind::Opaque(Box::new(OpaqueNode {
                tag: *tag,
                payload: Arc::from(&payload[..payload.len().min(256)]),
            })),
        }
    }

    fn attr(&self, a: &Attr) -> AttrValue {
        match a {
            Attr::LineWidth(w) => AttrValue::LineWidth(w.mp()),
            Attr::Winding(r) => AttrValue::WindingRule(match r % 4 {
                0 => FillRule::NonZero,
                1 => FillRule::EvenOdd,
                2 => FillRule::Positive,
                _ => FillRule::Negative,
            }),
            Attr::Join(j) => AttrValue::JoinType(match j % 3 {
                0 => Join::Mitre,
                1 => Join::Round,
                _ => Join::Bevel,
            }),
            Attr::Cap(c) => AttrValue::LineCap(match c % 3 {
                0 => Cap::Butt,
                1 => Cap::Round,
                _ => Cap::Square,
            }),
            Attr::MitreLimit(m) => AttrValue::MitreLimit(m.mp()),
            Attr::FontSize(s) => AttrValue::FontSize(s.mp()),
            Attr::AspectRatio(r) => AttrValue::AspectRatio(*r),
            Attr::Name(n) => AttrValue::ObjectName(Arc::from(n.chars().take(64).collect::<String>())),
            Attr::Clip(ops) => AttrValue::ClipRegion(Arc::new(common::build_path(ops))),
            Attr::Dash(elements, offset) => AttrValue::DashPattern(Arc::new(DashPattern {
                elements: elements.iter().take(8).map(|c| c.mp()).collect(),
                offset: offset.mp(),
                reference_width: None,
            })),
            Attr::FillIndexed(k, tint) => AttrValue::Fill(FillGeometry::Flat {
                value: Colour::Indexed {
                    id: self.colour(*k),
                    tint: *tint,
                },
            }),
            Attr::FillDirect(r, g, b, t) => AttrValue::Fill(FillGeometry::Flat {
                value: Colour::Direct(ColourValue::rgbt(*r, *g, *b, *t)),
            }),
        }
    }
}

fuzz_target!(|steps: Vec<Step>| {
    let mut b = DocumentBuilder::new(BuildLimits::small());
    let mut ctx = Ctx {
        colours: Vec::new(),
    };
    for step in steps.iter().take(MAX_STEPS) {
        let r: Result<(), BuildError> = match step {
            Step::Down => b.push_scope(),
            Step::Up => {
                b.pop_scope();
                Ok(())
            }
            Step::Node(k) => b.node(ctx.node(k)).map(|_| ()),
            Step::Attr(a) => b.attribute(ctx.attr(a)).map(|_| ()),
            Step::Default(a) => {
                b.default_attribute(ctx.attr(a));
                Ok(())
            }
            Step::Colour {
                parent,
                kind,
                components,
            } => {
                let def = ColourDef {
                    parent: parent.map(|p| ctx.colour(p)),
                    kind: ColourKind::from_byte(*kind, *components),
                    components: *components,
                    ..ColourDef::default()
                };
                let id = b.define_colour(def);
                ctx.colours.push(id);
                Ok(())
            }
        };
        if r.is_err() {
            // A limit or a refused relink: a legitimate way to stop.
            return;
        }
    }
    match b.finish() {
        Ok((doc, _)) => {
            let report = doc.validate();
            assert!(
                report.errors.is_empty(),
                "the builder returned an invalid document: {:#?}",
                report.errors
            );
        }
        Err(BuildError::Inconsistent(n)) => {
            panic!("the builder failed to repair its own document: {n} error(s)")
        }
        Err(_) => {}
    }
});
