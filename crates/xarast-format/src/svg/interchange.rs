//! The `Interchange` projection of the profile (phase 11 W11.3, T11.3.2).
//!
//! A standalone `.svg` for browsers and Inkscape carries the profile's
//! **base** representation and nothing of its parametric one: no `xarast:`
//! attribute, no `xarast:` element (with its subtree), no `xmlns:xarast`.
//! The mapper writes one document either way; [`project`] is the last step
//! of [`super::write_svg`] when the dialect is `Interchange`, so every
//! mapping decision (geometry, paint, ramp baking, masks, blend modes,
//! text placement) stays one code path shared with `.xarast`.
//!
//! The input is the writer's own output, so it is well-formed XML whose
//! attribute values are quoted and escaped; foreign baggage never reaches
//! it (the emitter drops baggage in this dialect). The scanner relies on
//! exactly that and is not a general XML parser.
//!
//! Dropping an element drops its subtree. That is right for every
//! `xarast:` element the writer produces: twins, palettes, the document
//! header, text items that are not drawn characters (`kern`, `eol`,
//! `char`), and opaque records, whose subtrees the renderer skips too.

use std::collections::HashSet;

/// What the projection removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionStats {
    /// `xarast:` elements dropped (each with its subtree).
    pub elements: usize,
    /// `xarast:` attributes dropped from kept elements.
    pub attributes: usize,
    /// `id` attributes dropped because nothing refers to them (minify).
    pub ids: usize,
}

/// The private prefix: every name of the parametric layer starts with it.
const PRIVATE: &str = "xarast:";

/// Projects profile SVG text onto its base representation.
///
/// With `minify`, `id`s nothing refers to (`#id` in an `href`, `url(#id)`
/// anywhere, CSS included) and comments are dropped as well, and the
/// pretty-printing indentation between elements is removed.
#[must_use]
pub fn project(svg: &str, minify: bool) -> (String, ProjectionStats) {
    let referenced = if minify {
        referenced_ids(svg)
    } else {
        HashSet::new()
    };
    let mut out = String::with_capacity(svg.len());
    let mut st = ProjectionStats::default();
    let b = svg.as_bytes();
    let mut i = 0usize;
    // Depth inside a dropped element; 0 when copying.
    let mut skip = 0usize;
    // Whether the last thing written was an element or a comment dropped
    // on its own line, so the line break after it goes too.
    let mut eat_newline = false;
    // Inside a `<text>`: whitespace there is content.
    let mut text_depth = 0usize;
    while i < b.len() {
        if b.get(i) != Some(&b'<') {
            let end = svg[i..].find('<').map_or(b.len(), |k| i + k);
            if skip == 0 {
                let mut seg = &svg[i..end];
                if eat_newline && let Some(rest) = seg.strip_prefix('\n') {
                    seg = rest;
                }
                if minify && text_depth == 0 && seg.trim().is_empty() {
                    seg = "";
                }
                out.push_str(seg);
            }
            eat_newline = false;
            i = end;
            continue;
        }
        let rest = &svg[i..];
        if rest.starts_with("<?") {
            let end = rest.find("?>").map_or(b.len(), |k| i + k + 2);
            if skip == 0 {
                out.push_str(&svg[i..end]);
            }
            i = end;
            continue;
        }
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(b.len(), |k| i + k + 3);
            if skip == 0 && !minify {
                out.push_str(&svg[i..end]);
            } else {
                eat_newline = out.ends_with('\n') || out.is_empty();
            }
            i = end;
            continue;
        }
        let end = tag_end(b, i);
        let tag = &svg[i..end];
        i = end;
        if let Some(name) = tag.strip_prefix("</") {
            let name = name.trim_end_matches('>').trim();
            if skip > 0 {
                skip -= 1;
                if skip == 0 {
                    eat_newline = out.ends_with('\n');
                }
                continue;
            }
            if name == "text" {
                text_depth = text_depth.saturating_sub(1);
            }
            out.push_str(tag);
            continue;
        }
        let self_closing = tag.ends_with("/>");
        let name_end = tag[1..]
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .map_or(tag.len(), |k| k + 1);
        let name = &tag[1..name_end];
        if skip > 0 {
            if !self_closing {
                skip += 1;
            }
            continue;
        }
        if name.starts_with(PRIVATE) {
            st.elements += 1;
            if self_closing {
                eat_newline = out.ends_with('\n');
            } else {
                skip = 1;
            }
            continue;
        }
        eat_newline = false;
        if name == "text" && !self_closing {
            text_depth += 1;
        }
        out.push('<');
        out.push_str(name);
        write_attrs(
            &tag[name_end..],
            minify.then_some(&referenced),
            &mut out,
            &mut st,
        );
        out.push_str(if self_closing { "/>" } else { ">" });
    }
    (out, st)
}

/// The byte after the `>` that closes the tag starting at `start`,
/// skipping quoted attribute values.
fn tag_end(b: &[u8], start: usize) -> usize {
    let mut quote = 0u8;
    let mut i = start + 1;
    while i < b.len() {
        let Some(&c) = b.get(i) else { break };
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'>' {
            return i + 1;
        }
        i += 1;
    }
    b.len()
}

/// Copies the attributes of a start tag (the text after its name), leaving
/// out private ones and, when `referenced` is given, unreferenced ids.
///
/// The profile writes an image's reference twice, as `href` (SVG 2) and
/// `xlink:href` (SVG 1.1, which the root declares); an interchange file
/// keeps only `xlink:href`, which every SVG 1.1 and SVG 2 reader takes, so
/// an inline `data:` image is not stored twice.
fn write_attrs(
    mut s: &str,
    referenced: Option<&HashSet<&str>>,
    out: &mut String,
    st: &mut ProjectionStats,
) {
    let mut attrs: Vec<(&str, char, &str)> = Vec::new();
    loop {
        s = s.trim_start();
        let Some(eq) = s.find('=') else { break };
        let name = s[..eq].trim();
        if name.is_empty() || name.starts_with('/') || name.starts_with('>') {
            break;
        }
        let after = s[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(q) else {
            break;
        };
        attrs.push((name, q, &after[1..=close]));
        s = &after[close + 2..];
    }
    let xlink = attrs
        .iter()
        .find(|(n, _, _)| *n == "xlink:href")
        .map(|(_, _, v)| *v);
    for (name, q, value) in attrs {
        if name.starts_with(PRIVATE) || name == "xmlns:xarast" {
            st.attributes += 1;
            continue;
        }
        if name == "href" && xlink == Some(value) {
            continue;
        }
        if name == "id"
            && let Some(r) = referenced
            && !r.contains(value)
        {
            st.ids += 1;
            continue;
        }
        out.push(' ');
        out.push_str(name);
        out.push('=');
        out.push(q);
        out.push_str(value);
        out.push(q);
    }
}

/// Every id the text refers to: `url(#id)` and `href="#id"`.
fn referenced_ids(svg: &str) -> HashSet<&str> {
    let mut ids = HashSet::new();
    let mut add = |pat: &str, stop: &[char]| {
        let mut from = 0usize;
        while let Some(k) = svg[from..].find(pat) {
            let start = from + k + pat.len();
            let end = svg[start..]
                .find(|c: char| stop.contains(&c))
                .map_or(svg.len(), |e| start + e);
            ids.insert(svg[start..end].trim());
            from = end;
        }
    };
    add("url(#", &[')']);
    add("href=\"#", &['"']);
    add("href='#", &['\'']);
    // `inkscape:current-layer` names a layer by id.
    add("inkscape:current-layer=\"", &['"']);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_vocabulary_goes_and_the_rest_stays() {
        let src = "<?xml version=\"1.0\"?>\n<svg xmlns=\"s\" xmlns:xarast=\"x\" id=\"r\">\n\
                   <defs>\n<xarast:document xarast:version=\"1.0\"/>\n\
                   <linearGradient id=\"g1\"><stop offset=\"0\"/></linearGradient>\n</defs>\n\
                   <path id=\"x1\" d=\"M0 0\" fill=\"url(#g1)\" xarast:shape=\"rect\">\
                   <xarast:twin a=\"1\"><xarast:inner/></xarast:twin></path>\n\
                   <text xml:space=\"preserve\"><tspan>a<xarast:kern xarast:em=\"1\"/>b</tspan></text>\n\
                   </svg>\n";
        let (out, st) = project(src, false);
        assert!(!out.contains("xarast"), "{out}");
        assert_eq!(
            out,
            "<?xml version=\"1.0\"?>\n<svg xmlns=\"s\" id=\"r\">\n<defs>\n\
             <linearGradient id=\"g1\"><stop offset=\"0\"/></linearGradient>\n</defs>\n\
             <path id=\"x1\" d=\"M0 0\" fill=\"url(#g1)\"></path>\n\
             <text xml:space=\"preserve\"><tspan>ab</tspan></text>\n</svg>\n"
        );
        assert_eq!(st.elements, 3);
        assert_eq!(st.attributes, 2);
        let (min, st) = project(src, true);
        assert!(min.contains("id=\"g1\""), "{min}");
        assert!(!min.contains("id=\"x1\""), "{min}");
        assert_eq!(st.ids, 2);
        // Idempotent.
        assert_eq!(project(&min, true).0, min);
    }

    #[test]
    fn an_image_reference_is_written_once() {
        let (out, _) = project(
            "<image href=\"data:x\" xlink:href=\"data:x\"/><use href=\"#a\"/>",
            false,
        );
        assert_eq!(out, "<image xlink:href=\"data:x\"/><use href=\"#a\"/>");
    }

    #[test]
    fn quoted_angle_brackets_do_not_end_a_tag() {
        let (out, _) = project("<a t=\"x&gt;y\" u='>'><b/></a>", false);
        assert_eq!(out, "<a t=\"x&gt;y\" u='>'><b/></a>");
    }
}
