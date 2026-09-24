//! `<xarast:photo-ops>`: a bitmap object's non-destructive photo
//! operations (`research/06 §6.9`, phase 10 W10.6).
//!
//! ```xml
//! <image href="resources/images/b3-….jpg" …>
//!   <xarast:photo-ops>
//!     <xarast:op xarast:kind="crop" xarast:rect="120 80 1800 1200"/>
//!     <xarast:op xarast:kind="orient" xarast:turns="1" xarast:flip="true"/>
//!     <xarast:op xarast:kind="levels" xarast:channel="rgb" xarast:input="10 240" xarast:output="0 255"/>
//!     <xarast:op xarast:kind="brightness" xarast:value="0.08"/>
//!     <xarast:op xarast:kind="greyscale"/>
//!   </xarast:photo-ops>
//! </image>
//! ```
//!
//! The `href` is the **master**; the object's placement maps the derived
//! image. Operations are written in the chain's order, which for an
//! editable chain is the canonical one. Numbers are Rust's shortest
//! round-trip spelling of an `f32`, so a reload gives the same bits. An
//! operation of a kind this version does not know is written back as the
//! text it was read as, character for character.

use xarast_doc::photo::{Levels, LevelsChannel, PhotoOp, PhotoOps, PhotoOrient, PixelRect};

/// The element for `ops`, or an empty string when there are none.
#[must_use]
pub fn write_photo_ops(ops: &PhotoOps) -> String {
    if ops.is_empty() {
        return String::new();
    }
    let mut s = String::from("<xarast:photo-ops>");
    for op in &ops.ops {
        if let PhotoOp::Unknown { raw, .. } = op {
            s.push_str(raw);
            continue;
        }
        s.push_str("<xarast:op xarast:kind=\"");
        s.push_str(op.kind_name());
        s.push('"');
        match op {
            PhotoOp::Crop(r) => {
                s.push_str(&format!(
                    " xarast:rect=\"{} {} {} {}\"",
                    r.x, r.y, r.width, r.height
                ));
            }
            PhotoOp::Orient(o) => {
                s.push_str(&format!(" xarast:turns=\"{}\"", o.turns % 4));
                if o.flip {
                    s.push_str(" xarast:flip=\"true\"");
                }
            }
            PhotoOp::Levels(l) => {
                s.push_str(&format!(
                    " xarast:channel=\"{}\" xarast:input=\"{} {}\" xarast:output=\"{} {}\"",
                    l.channel.name(),
                    l.in_lo,
                    l.in_hi,
                    l.out_lo,
                    l.out_hi
                ));
            }
            PhotoOp::Gamma(v)
            | PhotoOp::Brightness(v)
            | PhotoOp::Contrast(v)
            | PhotoOp::Saturation(v) => {
                s.push_str(&format!(" xarast:value=\"{v}\""));
            }
            PhotoOp::Greyscale | PhotoOp::Unknown { .. } => {}
        }
        s.push_str("/>");
    }
    s.push_str("</xarast:photo-ops>");
    s
}

/// One `<xarast:op>` of a known kind, from its `xarast:kind` and a lookup
/// of its other `xarast:` attributes. `None` when the kind is unknown or
/// a parameter is missing or malformed: the caller then keeps the
/// element verbatim as [`PhotoOp::Unknown`].
#[must_use]
pub fn parse_photo_op(kind: &str, attr: &dyn Fn(&str) -> Option<String>) -> Option<PhotoOp> {
    let nums = |name: &str| -> Option<Vec<u32>> {
        attr(name)?
            .split_ascii_whitespace()
            .map(|t| t.parse::<u32>().ok())
            .collect()
    };
    let value = || -> Option<f32> {
        let v: f32 = attr("value")?.trim().parse().ok()?;
        v.is_finite().then_some(v)
    };
    let byte = |v: u32| u8::try_from(v).ok();
    Some(match kind {
        "crop" => match nums("rect")?.as_slice() {
            &[x, y, width, height] => PhotoOp::Crop(PixelRect {
                x,
                y,
                width,
                height,
            }),
            _ => return None,
        },
        "orient" => {
            let turns = byte(attr("turns")?.trim().parse().ok()?)?;
            if turns > 3 {
                return None;
            }
            let flip = match attr("flip").as_deref() {
                None | Some("false") => false,
                Some("true") => true,
                Some(_) => return None,
            };
            PhotoOp::Orient(PhotoOrient { turns, flip })
        }
        "levels" => {
            let channel = LevelsChannel::from_name(attr("channel")?.trim())?;
            let (i, o) = (nums("input")?, nums("output")?);
            match (i.as_slice(), o.as_slice()) {
                (&[a, b], &[c, d]) => PhotoOp::Levels(Levels {
                    channel,
                    in_lo: byte(a)?,
                    in_hi: byte(b)?,
                    out_lo: byte(c)?,
                    out_hi: byte(d)?,
                }),
                _ => return None,
            }
        }
        "gamma" => PhotoOp::Gamma(value()?),
        "brightness" => PhotoOp::Brightness(value()?),
        "contrast" => PhotoOp::Contrast(value()?),
        "saturation" => PhotoOp::Saturation(value()?),
        "greyscale" => PhotoOp::Greyscale,
        _ => return None,
    })
}
