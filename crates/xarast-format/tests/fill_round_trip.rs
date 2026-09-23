//! Fills through `.xarast`: phase 8, W8.8 (T8.8.2, T8.8.3), acceptance
//! criterion 9.
//!
//! A document holding every fill shape — as a colour fill and as a
//! transparency — with a 7-stop ramp, non-identity profiles, the sine ramp
//! mapping, each fill effect and each fill mapping is saved, loaded and
//! saved again:
//!
//! - every loaded fill attribute compares **equal** to the original (not
//!   merely the same normal form), so the reader rebuilt ramps from their
//!   keys and discarded the baked stops (`research/06 §6.4`);
//! - the second save's `document.svg` is **byte-identical** to the first:
//!   re-baking a re-read curve gives the same stops, so no error
//!   accumulates over save/load cycles;
//! - every baked ramp in the file is within **2/255** per channel of the
//!   model's own curve, measured by interpolating the written stops
//!   linearly, as a browser does, at 4 097 points.

use std::io::Cursor;

use xarast_color::{Colour, ColourValue, FillEffect, Rgba8, TranspMode, Transparency};
use xarast_doc::attr::resolve_uncached;
use xarast_doc::fill::{Paint, Ramp, RampMapping, RampStop, Tiling, TranspPaint};
use xarast_doc::{AttrSlot, AttrValue, BuildLimits, Document, NodeKind, PathNode};
use xarast_format::svg::normal_form;
use xarast_format::{
    OpenOptions, SaveOptions, WriteOptions, XarastReader, open_reader, save_opened_to, save_to,
};
use xarast_geom::{BiasGain, Matrix, Path, Point, Vector};

/// A colour 8-bit sRGB holds exactly: what a key stop can carry.
fn rgb(r: u8, g: u8, b: u8) -> Colour {
    Colour::Direct(ColourValue::from_rgba8(Rgba8 { r, g, b, a: 255 }))
}

/// Seven intermediate stops, not evenly spaced.
fn ramp7<S: xarast_color::Stop>(value: impl Fn(usize) -> S, profile: BiasGain) -> Ramp<S> {
    let mut r = Ramp::new();
    for (i, pos) in [0.08f32, 0.2, 0.33, 0.5, 0.61, 0.8, 0.93]
        .into_iter()
        .enumerate()
    {
        r.insert(RampStop {
            pos,
            value: value(i),
        });
    }
    r.profile = profile;
    r
}

fn palette(i: usize) -> Colour {
    const C: [(u8, u8, u8); 7] = [
        (230, 25, 75),
        (60, 180, 75),
        (255, 225, 25),
        (0, 130, 200),
        (245, 130, 48),
        (145, 30, 180),
        (70, 240, 240),
    ];
    let (r, g, b) = C[i % C.len()];
    rgb(r, g, b)
}

fn level(i: usize) -> Transparency {
    Transparency {
        level: [10u8, 200, 40, 160, 90, 250, 0][i % 7],
        mode: TranspMode::Mix,
    }
}

fn pt(x: i32, y: i32) -> Point {
    Point::raw(x, y)
}

/// Every colour fill shape, with its ramp (where it has one) built by
/// `ramp`, around the box at `(x, y)`.
fn colour_fills(x: i32, y: i32, ramp: &dyn Fn() -> Ramp<Colour>) -> Vec<Paint> {
    let (a, b) = (rgb(20, 40, 60), rgb(250, 240, 200));
    vec![
        Paint::Flat {
            value: rgb(12, 34, 56),
        },
        Paint::Linear {
            start: pt(x, y),
            end: pt(x + 90_000, y + 30_000),
            persp: None,
            from: a.clone(),
            to: b.clone(),
            ramp: ramp(),
        },
        Paint::Radial {
            centre: pt(x + 50_000, y + 50_000),
            major: pt(x + 90_000, y + 50_000),
            minor: pt(x + 90_000, y + 50_000),
            aspect_locked: true,
            persp: None,
            from: a.clone(),
            to: b.clone(),
            ramp: ramp(),
        },
        Paint::Radial {
            centre: pt(x + 50_000, y + 50_000),
            major: pt(x + 95_000, y + 60_000),
            minor: pt(x + 44_000, y + 80_000),
            aspect_locked: false,
            persp: None,
            from: a.clone(),
            to: b.clone(),
            ramp: ramp(),
        },
        Paint::Conical {
            centre: pt(x + 50_000, y + 50_000),
            zero_dir: pt(x + 80_000, y + 70_000),
            from: a.clone(),
            to: b.clone(),
            ramp: ramp(),
        },
        Paint::Diamond {
            centre: pt(x + 50_000, y + 50_000),
            corner1: pt(x + 90_000, y + 55_000),
            corner2: pt(x + 45_000, y + 85_000),
            persp: None,
            from: a.clone(),
            to: b.clone(),
            ramp: ramp(),
        },
        Paint::ThreeColour {
            origin: pt(x, y),
            axis1: pt(x + 100_000, y),
            axis2: pt(x, y + 100_000),
            c0: a.clone(),
            c1: rgb(0, 200, 90),
            c2: b.clone(),
        },
        Paint::FourColour {
            origin: pt(x, y),
            axis1: pt(x + 100_000, y),
            axis2: pt(x, y + 100_000),
            axis3: pt(x + 100_000, y + 100_000),
            c0: a,
            c1: rgb(0, 200, 90),
            c2: rgb(200, 0, 90),
            c3: b,
        },
    ]
}

/// Every transparency shape SVG can mask or twin, likewise.
fn transparencies(x: i32, y: i32, ramp: &dyn Fn() -> Ramp<Transparency>) -> Vec<TranspPaint> {
    let t = |level: u8| Transparency {
        level,
        mode: TranspMode::Mix,
    };
    vec![
        TranspPaint::Linear {
            start: pt(x, y),
            end: pt(x + 100_000, y + 20_000),
            persp: None,
            from: t(0),
            to: t(220),
            ramp: ramp(),
        },
        TranspPaint::Radial {
            centre: pt(x + 50_000, y + 50_000),
            major: pt(x + 95_000, y + 60_000),
            minor: pt(x + 44_000, y + 80_000),
            aspect_locked: false,
            persp: None,
            from: t(30),
            to: t(250),
            ramp: ramp(),
        },
        TranspPaint::Conical {
            centre: pt(x + 50_000, y + 50_000),
            zero_dir: pt(x + 80_000, y + 30_000),
            from: t(0),
            to: t(255),
            ramp: ramp(),
        },
        TranspPaint::Diamond {
            centre: pt(x + 50_000, y + 50_000),
            corner1: pt(x + 90_000, y + 55_000),
            corner2: pt(x + 45_000, y + 85_000),
            persp: None,
            from: t(0),
            to: t(200),
            ramp: ramp(),
        },
        TranspPaint::ThreeColour {
            origin: pt(x, y),
            axis1: pt(x + 100_000, y),
            axis2: pt(x, y + 100_000),
            c0: t(0),
            c1: t(128),
            c2: t(250),
        },
        TranspPaint::FourColour {
            origin: pt(x, y),
            axis1: pt(x + 100_000, y),
            axis2: pt(x, y + 100_000),
            axis3: pt(x + 100_000, y + 100_000),
            c0: t(0),
            c1: t(128),
            c2: t(250),
            c3: t(60),
        },
    ]
}

fn square(x: i32, y: i32) -> NodeKind {
    let mut pb = Path::builder();
    pb.move_to(pt(x, y))
        .line_to(pt(x + 100_000, y))
        .line_to(pt(x + 100_000, y + 100_000))
        .line_to(pt(x, y + 100_000))
        .close();
    NodeKind::Path(Box::new(PathNode::new(pb.build())))
}

/// The document: one square per case, each carrying its own attributes.
fn fixture() -> (Document, usize) {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    let mut cases: Vec<Vec<AttrValue>> = Vec::new();
    let profiles = [
        BiasGain::IDENTITY,
        BiasGain::new(0.35, 0.0),
        BiasGain::new(-0.4, 0.6),
    ];
    let effects = [
        FillEffect::Fade,
        FillEffect::Rainbow,
        FillEffect::AltRainbow,
    ];
    let tilings = [
        Tiling::None,
        Tiling::Simple,
        Tiling::Repeat,
        Tiling::RepeatInverted,
        Tiling::RepeatExtra,
    ];
    let mut n = 0usize;
    for (pi, profile) in profiles.into_iter().enumerate() {
        for (ei, effect) in effects.into_iter().enumerate() {
            let sin = (pi + ei) % 2 == 1;
            let ramp = || {
                let mut r = ramp7(palette, profile);
                if sin {
                    r.mapping = RampMapping::Sin;
                }
                r
            };
            for fill in colour_fills(0, 0, &ramp) {
                // A flat fill has no ramp: an effect or mapping beside it
                // modifies nothing and is not written.
                let flat = matches!(fill, Paint::Flat { .. });
                let mut attrs = vec![AttrValue::Fill(fill)];
                if effect != FillEffect::Fade && !flat {
                    attrs.push(AttrValue::FillEffect(effect));
                }
                let tiling = tilings[n % tilings.len()];
                if tiling != Tiling::None && !flat {
                    attrs.push(AttrValue::FillMapping(tiling));
                }
                cases.push(attrs);
                n += 1;
            }
        }
        let tramp = || {
            let mut r = ramp7(level, profile);
            if pi == 1 {
                r.mapping = RampMapping::Sin;
            }
            r
        };
        for t in transparencies(0, 0, &tramp) {
            cases.push(vec![
                AttrValue::Fill(Paint::Flat {
                    value: rgb(90, 20, 160),
                }),
                AttrValue::TranspFill(t),
            ]);
        }
    }
    let count = cases.len();
    for (i, attrs) in cases.into_iter().enumerate() {
        let (x, y) = ((i % 12) as i32 * 120_000, (i / 12) as i32 * 120_000);
        b.node(square(x, y)).unwrap();
        b.push_scope().unwrap();
        for mut a in attrs {
            // Move every control point with the square.
            a.transform(Matrix::translate(Vector::raw(x, y)));
            b.attribute(a).unwrap();
        }
        b.pop_scope();
    }
    (b.finish().unwrap().0, count)
}

fn deterministic() -> SaveOptions {
    SaveOptions {
        write: WriteOptions::deterministic(),
        ..SaveOptions::default()
    }
}

fn svg_of(bytes: &[u8]) -> String {
    let mut r = XarastReader::open(Cursor::new(bytes.to_vec())).unwrap();
    String::from_utf8(r.document_bytes().unwrap()).unwrap()
}

/// The fill slots in force on the `n`th path.
fn slots(doc: &Document, n: usize) -> Vec<AttrValue> {
    let id = doc
        .tree
        .preorder(doc.tree.root())
        .filter(|i| matches!(doc.tree.kind(*i), Some(NodeKind::Path(_))))
        .nth(n)
        .expect("path");
    let r = resolve_uncached(&doc.tree, id, &doc.defaults);
    [
        AttrSlot::FillGeometry,
        AttrSlot::FillMapping,
        AttrSlot::FillEffect,
        AttrSlot::TranspFillGeometry,
        AttrSlot::TranspFillMapping,
    ]
    .into_iter()
    .map(|s| r.get(s).clone())
    .collect()
}

#[test]
fn every_fill_survives_save_load_save_exactly_and_the_second_save_is_a_fixed_point() {
    let (doc, cases) = fixture();
    let mut first = Cursor::new(Vec::new());
    save_to(&doc, &mut first, &deterministic()).unwrap();
    let first = first.into_inner();
    let mut opened = open_reader(Cursor::new(first.clone()), &OpenOptions::default()).unwrap();
    assert!(opened.diagnostics.is_empty(), "{:?}", opened.diagnostics);
    for i in 0..cases {
        assert_eq!(slots(&doc, i), slots(&opened.document, i), "case {i}");
    }
    assert_eq!(normal_form(&doc), normal_form(&opened.document));
    let mut second = Cursor::new(Vec::new());
    save_opened_to(
        &opened.document,
        &mut opened.package,
        &mut second,
        &deterministic(),
    )
    .unwrap();
    let second = second.into_inner();
    let (a, b) = (svg_of(&first), svg_of(&second));
    assert!(a == b, "document.svg changed on the second save");
    assert_eq!(first, second, "the whole package is a fixed point");
    // The document really exercises the baking.
    assert!(a.contains("xarast:profile="), "no profile written");
    assert!(a.contains("xarast:ramp-mapping=\"sin\""), "no sin mapping");
    assert!(a.contains("xarast:fill-effect=\"alt-rainbow\""));
}

/// `(offset, colour)` of every `<stop>` in one gradient element.
fn stops(gradient: &str) -> Vec<(f64, [f64; 4])> {
    let mut out = Vec::new();
    for s in gradient.split("<stop").skip(1) {
        let s = s.split("/>").next().unwrap();
        let attr = |name: &str| -> Option<&str> {
            let k = format!(" {name}=\"");
            let at = s.find(&k)? + k.len();
            Some(&s[at..at + s[at..].find('"')?])
        };
        let offset: f64 = attr("offset").unwrap().parse().unwrap();
        let hex = attr("stop-color").unwrap().trim_start_matches('#');
        let hex = if hex.len() == 3 {
            hex.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            hex.to_owned()
        };
        let ch = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
        let a = attr("stop-opacity").map_or(255.0, |o| o.parse::<f64>().unwrap() * 255.0);
        out.push((offset, [ch(0), ch(2), ch(4), a]));
    }
    out
}

fn lerp_stops(s: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    let i = s.partition_point(|(o, _)| *o <= t).clamp(1, s.len() - 1);
    let ((o0, c0), (o1, c1)) = (s[i - 1], s[i]);
    let u = if o1 > o0 {
        ((t - o0) / (o1 - o0)).clamp(0.0, 1.0)
    } else {
        1.0
    };
    std::array::from_fn(|k| c0[k] + (c1[k] - c0[k]) * u)
}

#[test]
fn every_baked_ramp_is_within_two_levels_of_the_model_curve() {
    let (doc, _) = fixture();
    let mut out = Cursor::new(Vec::new());
    save_to(&doc, &mut out, &deterministic()).unwrap();
    let svg = svg_of(&out.into_inner());
    let mut checked = 0usize;
    let mut worst = 0.0f64;
    for g in svg
        .split("<linearGradient")
        .skip(1)
        .chain(svg.split("<radialGradient").skip(1))
    {
        let head = g.split('>').next().unwrap();
        let Some(keys) = head
            .split("xarast:stops=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
        else {
            continue;
        };
        if !head.contains("xarast:profile")
            && !head.contains("xarast:ramp-mapping")
            && !head.contains("xarast:fill-effect")
        {
            continue;
        }
        // Rebuild the model ramp from the twin.
        let attr = |name: &str| {
            head.split(&format!("{name}=\""))
                .nth(1)
                .and_then(|s| s.split('"').next())
        };
        let profile = attr("xarast:profile").map_or(BiasGain::IDENTITY, |p| {
            let mut it = p.split(' ').map(|v| v.parse::<f64>().unwrap());
            BiasGain::new(it.next().unwrap(), it.next().unwrap())
        });
        let effect = match attr("xarast:fill-effect") {
            Some("rainbow") => FillEffect::Rainbow,
            Some("alt-rainbow") => FillEffect::AltRainbow,
            _ => FillEffect::Fade,
        };
        let key: Vec<(f32, ColourValue)> = keys
            .split(' ')
            .map(|k| {
                let (p, c) = k.split_once(':').unwrap();
                let c = c.trim_start_matches('#');
                let ch = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap();
                (
                    p.parse().unwrap(),
                    ColourValue::from_rgba8(Rgba8 {
                        r: ch(0),
                        g: ch(2),
                        b: ch(4),
                        a: 255,
                    }),
                )
            })
            .collect();
        let mut ramp: Ramp<ColourValue> = Ramp::new();
        for (p, c) in &key[1..key.len() - 1] {
            ramp.insert(RampStop { pos: *p, value: *c });
        }
        ramp.profile = profile;
        if attr("xarast:ramp-mapping") == Some("sin") {
            ramp.mapping = RampMapping::Sin;
        }
        let (from, to) = (key[0].1, key[key.len() - 1].1);
        let baked = stops(g.split("Gradient>").next().unwrap());
        for i in 0..=4096 {
            let t = f64::from(i) / 4096.0;
            let want = ramp.sample(&from, &to, t as f32, effect).to_rgba8();
            let got = lerp_stops(&baked, t);
            let want = [want.r, want.g, want.b, want.a].map(f64::from);
            for k in 0..4 {
                worst = worst.max((got[k] - want[k]).abs());
            }
        }
        checked += 1;
    }
    assert!(checked >= 20, "{checked} baked ramps checked");
    assert!(worst <= 2.0, "worst baked-ramp error {worst}/255");
}
