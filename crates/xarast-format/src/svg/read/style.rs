//! Presentation properties: attributes, `style`, `<style>` classes and
//! inheritance.
//!
//! The writer puts every ink element's resolved paint on the element
//! itself, but the reader must not rely on that: attribute hoisting and CSS
//! classes (passes 4–5 of `research/06 §4.5.1`) move paint to ancestors and
//! to a stylesheet, and third-party editors (Inkscape) rewrite paint into
//! `style`. So the reader computes each property the way SVG does: the
//! `style` attribute, then class rules, then the presentation attribute,
//! then — for inherited properties — the parent's computed value.

use std::sync::Arc;

use super::dom::Elem;

/// Every property the reader interprets, and whether it inherits.
const PROPS: &[(&str, bool)] = &[
    ("fill", true),
    ("fill-opacity", true),
    ("fill-rule", true),
    ("stroke", true),
    ("stroke-width", true),
    ("stroke-opacity", true),
    ("stroke-linecap", true),
    ("stroke-linejoin", true),
    ("stroke-miterlimit", true),
    ("stroke-dasharray", true),
    ("stroke-dashoffset", true),
    ("font-family", true),
    ("font-size", true),
    ("font-weight", true),
    ("font-style", true),
    ("text-anchor", true),
    ("color", true),
    ("color-interpolation", true),
    ("visibility", true),
    ("text-decoration", false),
    ("opacity", false),
    ("mask", false),
    ("clip-path", false),
    ("mix-blend-mode", false),
    ("display", false),
    ("vector-effect", false),
    // The palette twins of `fill` and `stroke`: attributes in the `xarast`
    // namespace, inherited like the properties they shadow and carried by
    // classes through `<xarast:paint-class>` (the pass 4–5 contract,
    // XARA-T-0107).
    (FILL_REF, true),
    (STROKE_REF, true),
];

/// The pseudo-property of `xarast:fill-ref`.
pub(crate) const FILL_REF: &str = "xarast:fill-ref";
/// The pseudo-property of `xarast:stroke-ref`.
pub(crate) const STROKE_REF: &str = "xarast:stroke-ref";

/// Whether `name` is a presentation property the reader interprets.
pub(crate) fn is_known_property(name: &str) -> bool {
    !name.starts_with("xarast:") && PROPS.iter().any(|(p, _)| *p == name)
}

fn index_of(name: &str) -> Option<usize> {
    PROPS.iter().position(|(p, _)| *p == name)
}

/// The computed values of one element.
#[derive(Debug, Clone, Default)]
pub(crate) struct Computed {
    values: Vec<Option<Arc<str>>>,
}

impl Computed {
    /// A property's computed value.
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        index_of(name)
            .and_then(|i| self.values.get(i))
            .and_then(|v| v.as_deref())
    }
}

/// The class rules of every `<style>` element.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stylesheet {
    /// `(class, declarations)` in source order; later wins.
    rules: Vec<(String, Vec<(String, String)>)>,
    /// Selectors the reader could not use (anything but `.class`).
    pub unsupported: usize,
    /// The palette twins of each class, from `<xarast:paint-class>`.
    twins: Vec<(String, Option<String>, Option<String>)>,
}

/// Splits a declaration block: `a:b; c:d`.
pub(crate) fn declarations(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in s.split(';') {
        let Some((k, v)) = d.split_once(':') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim().trim_end_matches("!important").trim();
        if k.is_empty() {
            continue;
        }
        out.push((k.to_ascii_lowercase(), v.to_owned()));
    }
    out
}

impl Stylesheet {
    /// Adds the rules of one stylesheet. Comments are dropped; only class
    /// selectors (`.a`, `.a,.b`) are understood.
    pub(crate) fn add(&mut self, css: &str) {
        let mut text = String::with_capacity(css.len());
        let mut rest = css;
        while let Some(i) = rest.find("/*") {
            text.push_str(rest.get(..i).unwrap_or_default());
            rest = match rest.get(i..).and_then(|r| r.find("*/")) {
                Some(j) => rest
                    .get(i.saturating_add(j).saturating_add(2)..)
                    .unwrap_or_default(),
                None => "",
            };
        }
        text.push_str(rest);
        let mut rest = text.as_str();
        while let Some(open) = rest.find('{') {
            let Some(close) = rest.find('}') else { break };
            if close < open {
                rest = rest.get(close.saturating_add(1)..).unwrap_or_default();
                continue;
            }
            let selectors = rest.get(..open).unwrap_or_default();
            let body = rest.get(open.saturating_add(1)..close).unwrap_or_default();
            // `@font-face` (the embedded fonts, `research/06 §6.7` rule 2)
            // is for browsers; the model's fonts come from the runs.
            if selectors.trim_start().starts_with("@font-face") {
                rest = rest.get(close.saturating_add(1)..).unwrap_or_default();
                continue;
            }
            let decls = declarations(body);
            for sel in selectors.split(',') {
                let sel = sel.trim();
                match sel.strip_prefix('.') {
                    Some(c)
                        if !c.is_empty()
                            && c.chars()
                                .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_') =>
                    {
                        self.rules.push((c.to_owned(), decls.clone()));
                    }
                    _ => self.unsupported = self.unsupported.saturating_add(1),
                }
            }
            rest = rest.get(close.saturating_add(1)..).unwrap_or_default();
        }
    }

    /// Records the twins of a class (`<xarast:paint-class>`).
    pub(crate) fn add_twins(&mut self, class: &str, fill: Option<&str>, stroke: Option<&str>) {
        self.twins.push((
            class.to_owned(),
            fill.map(str::to_owned),
            stroke.map(str::to_owned),
        ));
    }

    /// Whether there are no rules.
    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.twins.is_empty()
    }

    /// Whether every class in a `class` attribute is one of ours: then the
    /// attribute is consumed; otherwise it is kept as foreign data.
    pub(crate) fn knows_all(&self, classes: &str) -> bool {
        classes.split_ascii_whitespace().all(|c| {
            self.rules.iter().any(|(r, _)| r == c) || self.twins.iter().any(|(r, _, _)| r == c)
        })
    }
}

/// Computes an element's properties from its parent's.
///
/// Returns the computed values and the `style` declarations the reader did
/// not interpret, to be kept as foreign data.
pub(crate) fn compute(
    e: &Elem,
    parent: &Computed,
    sheet: &Stylesheet,
) -> (Computed, Vec<(String, String)>) {
    let mut values: Vec<Option<Arc<str>>> = vec![None; PROPS.len()];
    // Lowest priority first: inheritance, attribute, classes, style.
    for (i, (_, inherited)) in PROPS.iter().enumerate() {
        if *inherited && let Some(slot) = values.get_mut(i) {
            *slot = parent.values.get(i).cloned().flatten();
        }
    }
    let set = |name: &str, v: &str, values: &mut Vec<Option<Arc<str>>>| -> bool {
        let Some(i) = index_of(name) else {
            return false;
        };
        let v = v.trim();
        let value = if v == "inherit" {
            parent.values.get(i).cloned().flatten()
        } else {
            Some(Arc::from(v))
        };
        if let Some(slot) = values.get_mut(i) {
            *slot = value;
        }
        true
    };
    for a in &e.attrs {
        if a.ns.is_empty() {
            if a.local.starts_with("xarast:") {
                continue;
            }
            set(&a.local, &a.value, &mut values);
        } else if &*a.ns == crate::svg::NS_XARAST {
            match &*a.local {
                "fill-ref" => {
                    set(FILL_REF, &a.value, &mut values);
                }
                "stroke-ref" => {
                    set(STROKE_REF, &a.value, &mut values);
                }
                _ => {}
            }
        }
    }
    if !sheet.is_empty()
        && let Some(classes) = e.get("", "class")
    {
        let has = |name: &str| classes.split_ascii_whitespace().any(|c| c == name);
        for (class, decls) in &sheet.rules {
            if has(class) {
                for (k, v) in decls {
                    if !k.starts_with("xarast:") {
                        set(k, v, &mut values);
                    }
                }
            }
        }
        for (class, fill, stroke) in &sheet.twins {
            if has(class) {
                if let Some(f) = fill {
                    set(FILL_REF, f, &mut values);
                }
                if let Some(s) = stroke {
                    set(STROKE_REF, s, &mut values);
                }
            }
        }
    }
    let mut leftover = Vec::new();
    if let Some(style) = e.get("", "style") {
        for (k, v) in declarations(style) {
            if k.starts_with("xarast:") || !set(&k, &v, &mut values) {
                leftover.push((k, v));
            }
        }
    }
    (Computed { values }, leftover)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::read::dom::{XmlLimits, parse};

    #[test]
    fn cascade_and_inheritance() {
        let s = r##"<svg xmlns="http://www.w3.org/2000/svg"><style>.a{fill:#00f} /* x */ g > p{fill:red}</style><g fill="#0f0" stroke-width="2"><path class="a" style="stroke:#f00;foo:bar" fill="#fff"/><path/></g></svg>"##;
        let dom = parse(
            s.as_bytes(),
            XmlLimits {
                max_depth: 10,
                max_elements: 10,
                max_bytes: 1000,
            },
        )
        .unwrap();
        let mut sheet = Stylesheet::default();
        sheet.add(&dom.elem(1).unwrap().text());
        assert_eq!(sheet.unsupported, 1);
        let root = Computed::default();
        let (g, _) = compute(dom.elem(2).unwrap(), &root, &sheet);
        let (p, left) = compute(dom.elem(3).unwrap(), &g, &sheet);
        assert_eq!(p.get("fill"), Some("#00f"));
        assert_eq!(p.get("stroke"), Some("#f00"));
        assert_eq!(p.get("stroke-width"), Some("2"));
        assert_eq!(left, vec![("foo".to_owned(), "bar".to_owned())]);
        let (q, _) = compute(dom.elem(4).unwrap(), &g, &sheet);
        assert_eq!(q.get("fill"), Some("#0f0"));
        assert_eq!(q.get("opacity"), None);
    }

    #[test]
    fn font_face_rules_are_skipped_quietly() {
        let mut sheet = Stylesheet::default();
        sheet.add(
            "@font-face{font-family:'Noto Sans';font-weight:400;font-style:normal;\
             src:url(data:font/woff2;base64,d09GMg==) format('woff2');}\n.c1{fill:#f00}",
        );
        assert_eq!(sheet.unsupported, 0);
        assert!(sheet.knows_all("c1"));
    }
}
