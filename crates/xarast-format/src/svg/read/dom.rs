//! The preserving XML layer of the reader (F4.1).
//!
//! `document.svg` is parsed into a small arena of elements that keeps what
//! the model needs **and** what preservation needs: every element's byte
//! span in the input (a foreign fragment is the exact text that was read),
//! its own namespace declarations (to make a fragment namespace-complete),
//! comments and processing instructions with their spans, and character
//! data with the entity and character references resolved.
//!
//! The configuration is the one `research/06 §5.3` and `§10.5` demand:
//! end names checked, no trimming, no entity expansion beyond the five
//! predefined ones and character references, any DOCTYPE rejected, UTF-8
//! only, every name validated as a QName (quick-xml does not), every prefix
//! bound, nesting and element counts bounded before anything is allocated
//! for them.

#![deny(clippy::arithmetic_side_effects)]

use std::collections::HashMap;
use std::sync::Arc;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::PrefixDeclaration;

use crate::svg::NS_XML;
use crate::svg::xml::{is_ncname, is_xml_char};

/// Why the XML could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XmlError {
    /// Not UTF-8, or an encoding declaration naming something else.
    #[error("document.svg is not UTF-8")]
    NotUtf8,
    /// A DOCTYPE: rejected outright (`research/06 §5.3`, XXE).
    #[error("document.svg has a DOCTYPE, which the profile forbids")]
    Dtd,
    /// An entity reference other than the five predefined ones.
    #[error("undefined entity &{0};")]
    UndefinedEntity(String),
    /// Nesting deeper than the limit.
    #[error("XML nested deeper than {0} levels")]
    TooDeep(u32),
    /// More elements than the limit.
    #[error("more than {0} XML elements")]
    TooManyElements(usize),
    /// The input is larger than the limit.
    #[error("document.svg is larger than {0} bytes")]
    TooLarge(usize),
    /// A prefix with no binding.
    #[error("unbound namespace prefix {0:?}")]
    UnboundPrefix(String),
    /// A character XML 1.0 forbids.
    #[error("a character XML forbids")]
    InvalidCharacter,
    /// No root element, or something other than one element at the top.
    #[error("not a single-rooted XML document")]
    NotADocument,
    /// Anything else quick-xml or the name checks reject.
    #[error("malformed XML at byte {offset}: {message}")]
    Syntax {
        /// Where, in bytes from the start.
        offset: usize,
        /// What.
        message: String,
    },
}

/// Limits the XML layer enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlLimits {
    /// Maximum nesting depth (`research/06 §10.5`: 256).
    pub max_depth: u32,
    /// Maximum number of elements.
    pub max_elements: usize,
    /// Maximum input size in bytes.
    pub max_bytes: usize,
}

/// One attribute, names resolved.
#[derive(Debug, Clone)]
pub(crate) struct Attr {
    /// Namespace URI; empty for an unprefixed attribute.
    pub ns: Arc<str>,
    /// The prefix it was written with.
    pub prefix: Option<Arc<str>>,
    /// Local name.
    pub local: Box<str>,
    /// Value after attribute-value normalisation, references resolved.
    pub value: Box<str>,
}

/// A child of an element.
#[derive(Debug, Clone)]
pub(crate) enum Child {
    /// An element, by index into [`Dom::elems`].
    Elem(usize),
    /// Character data (text, CDATA and references), decoded.
    Text(Box<str>),
    /// A comment: its span in the input.
    Comment(usize, usize),
    /// A processing instruction: its span.
    Pi(usize, usize),
}

/// One element.
#[derive(Debug, Clone)]
pub(crate) struct Elem {
    /// Namespace URI; empty when none.
    pub ns: Arc<str>,
    /// The prefix of the element name.
    pub prefix: Option<Arc<str>>,
    /// Local name.
    pub local: Box<str>,
    /// Attributes, namespace declarations excluded.
    pub attrs: Vec<Attr>,
    /// Namespace declarations made on this element: `("", uri)` for the
    /// default namespace.
    pub decls: Vec<(Arc<str>, Arc<str>)>,
    /// Children in document order.
    pub children: Vec<Child>,
    /// Parent element.
    pub parent: Option<usize>,
    /// Byte span of the whole element, start tag to end tag inclusive.
    pub start: usize,
    /// End of the span (exclusive).
    pub end: usize,
}

impl Elem {
    /// The value of the attribute `local` in namespace `ns`.
    pub fn get(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| &*a.ns == ns && &*a.local == local)
            .map(|a| &*a.value)
    }

    /// Whether the element is `local` in namespace `ns`.
    pub fn is(&self, ns: &str, local: &str) -> bool {
        &*self.ns == ns && &*self.local == local
    }

    /// The concatenated character data of the element's direct children.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            if let Child::Text(t) = c {
                s.push_str(t);
            }
        }
        s
    }
}

/// A comment or PI outside the root element.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Misc {
    /// A comment span.
    Comment(usize, usize),
    /// A processing-instruction span.
    Pi(usize, usize),
}

/// The parsed document.
#[derive(Debug)]
pub(crate) struct Dom<'a> {
    /// The input, BOM stripped: spans index into it.
    pub input: &'a str,
    /// Every element; the root is the first.
    pub elems: Vec<Elem>,
    /// Comments and PIs before and after the root element.
    pub misc: Vec<Misc>,
}

impl<'a> Dom<'a> {
    /// The root element.
    pub fn root(&self) -> Option<&Elem> {
        self.elems.first()
    }

    /// An element by index.
    pub fn elem(&self, i: usize) -> Option<&Elem> {
        self.elems.get(i)
    }

    /// The text of a span.
    pub fn span(&self, start: usize, end: usize) -> &'a str {
        self.input.get(start..end).unwrap_or_default()
    }

    /// The namespace URI `prefix` is bound to where `at` is.
    fn binding(&self, mut at: Option<usize>, prefix: &str) -> Option<Arc<str>> {
        while let Some(i) = at {
            let e = self.elems.get(i)?;
            if let Some((_, u)) = e.decls.iter().rev().find(|(p, _)| &**p == prefix) {
                return Some(Arc::clone(u));
            }
            at = e.parent;
        }
        None
    }

    /// The element `i` as a standalone fragment: its exact text, with the
    /// namespace declarations it uses but inherits inserted into its start
    /// tag. Only the bindings it **uses**: inserting every binding in scope
    /// would make the second save differ from the first (see the manifest's
    /// fuzz finding in `docs/memory/xarast-format.md`).
    pub fn fragment(&self, i: usize) -> Option<String> {
        self.fragment_with(i, false)
    }

    /// [`Dom::fragment`] for a fragment that is written back inside a
    /// Xarast element: the `xarast` prefix, when it is bound to the Xarast
    /// namespace, is declared by every document the writer makes, so it
    /// is not inserted — the fragment's text stays exactly as read (a
    /// preserved photo operation, `research/06 §8.7`).
    pub fn fragment_in_xarast(&self, i: usize) -> Option<String> {
        self.fragment_with(i, true)
    }

    fn fragment_with(&self, i: usize, omit_xarast: bool) -> Option<String> {
        let e = self.elems.get(i)?;
        let raw = self.span(e.start, e.end);
        let mut used: Vec<Arc<str>> = Vec::new();
        let mut stack = vec![i];
        let mut guard = 0usize;
        while let Some(j) = stack.pop() {
            guard = guard.saturating_add(1);
            if guard > self.elems.len() {
                break;
            }
            let Some(d) = self.elems.get(j) else { continue };
            let p: Arc<str> = d.prefix.clone().unwrap_or_else(|| Arc::from(""));
            if !used.contains(&p) {
                used.push(p);
            }
            for a in &d.attrs {
                if let Some(p) = &a.prefix
                    && !used.contains(p)
                {
                    used.push(Arc::clone(p));
                }
            }
            for c in &d.children {
                if let Child::Elem(k) = c {
                    stack.push(*k);
                }
            }
        }
        used.sort();
        let mut decls = String::new();
        for p in used {
            if &*p == "xml" || e.decls.iter().any(|(q, _)| *q == p) {
                continue;
            }
            let Some(uri) = self.binding(e.parent, &p) else {
                continue;
            };
            if omit_xarast && &*p == "xarast" && &*uri == crate::svg::NS_XARAST {
                continue;
            }
            if p.is_empty() {
                // Fragments are only ever written back inside a Xarast
                // document, whose default namespace is SVG: declaring it
                // would change the fragment's text and nothing else.
                if uri.is_empty() || &*uri == crate::svg::NS_SVG {
                    continue;
                }
                decls.push_str(" xmlns=\"");
            } else {
                decls.push_str(" xmlns:");
                decls.push_str(&p);
                decls.push_str("=\"");
            }
            crate::svg::xml::push_attr_escaped(&mut decls, &uri);
            decls.push('"');
        }
        if decls.is_empty() {
            return Some(raw.to_owned());
        }
        // The element name ends at the first whitespace, `/` or `>`.
        let name_end = raw
            .char_indices()
            .skip(1)
            .find(|(_, c)| c.is_ascii_whitespace() || *c == '/' || *c == '>')
            .map_or(raw.len(), |(k, _)| k);
        let (head, tail) = raw.split_at(name_end);
        let mut out = String::with_capacity(raw.len().saturating_add(decls.len()));
        out.push_str(head);
        out.push_str(&decls);
        out.push_str(tail);
        Some(out)
    }
}

fn syntax(offset: usize, message: impl std::fmt::Display) -> XmlError {
    XmlError::Syntax {
        offset,
        message: message.to_string(),
    }
}

/// Splits and validates a qualified name.
fn split_qname(q: &str, offset: usize) -> Result<(&str, &str), XmlError> {
    let (prefix, local) = q.split_once(':').unwrap_or(("", q));
    let prefix_ok = !q.contains(':') || is_ncname(prefix);
    if prefix_ok && is_ncname(local) {
        Ok((prefix, local))
    } else {
        Err(syntax(offset, format!("invalid name {q:?}")))
    }
}

struct Builder {
    limits: XmlLimits,
    elems: Vec<Elem>,
    open: Vec<usize>,
    misc: Vec<Misc>,
    interned: HashMap<Box<str>, Arc<str>>,
    empty: Arc<str>,
    root_closed: bool,
}

impl Builder {
    fn intern(&mut self, s: &str) -> Arc<str> {
        if s.is_empty() {
            return Arc::clone(&self.empty);
        }
        if let Some(a) = self.interned.get(s) {
            return Arc::clone(a);
        }
        let a: Arc<str> = Arc::from(s);
        self.interned.insert(Box::from(s), Arc::clone(&a));
        a
    }

    fn resolve(&self, decls: &[(Arc<str>, Arc<str>)], prefix: &str) -> Option<Arc<str>> {
        if prefix == "xml" {
            return Some(Arc::from(NS_XML));
        }
        if let Some((_, u)) = decls.iter().rev().find(|(p, _)| &**p == prefix) {
            return Some(Arc::clone(u));
        }
        for &i in self.open.iter().rev() {
            if let Some((_, u)) = self
                .elems
                .get(i)
                .and_then(|e| e.decls.iter().rev().find(|(p, _)| &**p == prefix))
            {
                return Some(Arc::clone(u));
            }
        }
        None
    }

    fn start(&mut self, e: &BytesStart<'_>, start: usize) -> Result<usize, XmlError> {
        if self.root_closed || (self.open.is_empty() && !self.elems.is_empty()) {
            return Err(XmlError::NotADocument);
        }
        if self.elems.len() >= self.limits.max_elements {
            return Err(XmlError::TooManyElements(self.limits.max_elements));
        }
        let depth = u32::try_from(self.open.len()).unwrap_or(u32::MAX);
        if depth >= self.limits.max_depth {
            return Err(XmlError::TooDeep(self.limits.max_depth));
        }
        let mut decls: Vec<(Arc<str>, Arc<str>)> = Vec::new();
        let mut raw: Vec<(String, String)> = Vec::new();
        for a in e.attributes().with_checks(true) {
            let a = a.map_err(|err| syntax(start, err))?;
            let value = a
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|err| match err {
                    quick_xml::Error::Escape(
                        quick_xml::escape::EscapeError::UnrecognizedEntity(_, name),
                    ) => XmlError::UndefinedEntity(name),
                    other => syntax(start, other),
                })?
                .into_owned();
            if !value.chars().all(is_xml_char) {
                return Err(XmlError::InvalidCharacter);
            }
            match a.key.as_namespace_binding() {
                Some(PrefixDeclaration::Default) => {
                    let u = self.intern(&value);
                    decls.push((Arc::clone(&self.empty), u));
                }
                Some(PrefixDeclaration::Named(p)) => {
                    let p = std::str::from_utf8(p).map_err(|_| XmlError::NotUtf8)?;
                    if !is_ncname(p)
                        || p == "xmlns"
                        || (p == "xml" && value != NS_XML)
                        || (p != "xml" && value == NS_XML)
                        || value.is_empty()
                    {
                        return Err(syntax(start, format!("bad namespace declaration {p:?}")));
                    }
                    let (p, u) = (self.intern(p), self.intern(&value));
                    decls.push((p, u));
                }
                None => {
                    let k = std::str::from_utf8(a.key.as_ref()).map_err(|_| XmlError::NotUtf8)?;
                    raw.push((k.to_owned(), value));
                }
            }
        }
        let qname = std::str::from_utf8(e.name().as_ref())
            .map_err(|_| XmlError::NotUtf8)?
            .to_owned();
        let (prefix, local) = split_qname(&qname, start)?;
        let ns = match self.resolve(&decls, prefix) {
            Some(u) => u,
            None if prefix.is_empty() => Arc::clone(&self.empty),
            None => return Err(XmlError::UnboundPrefix(prefix.to_owned())),
        };
        let mut attrs = Vec::with_capacity(raw.len());
        for (k, value) in raw {
            let (p, l) = split_qname(&k, start)?;
            let ans = if p.is_empty() {
                Arc::clone(&self.empty)
            } else {
                self.resolve(&decls, p)
                    .ok_or_else(|| XmlError::UnboundPrefix(p.to_owned()))?
            };
            if attrs.iter().any(|x: &Attr| x.ns == ans && &*x.local == l) {
                return Err(syntax(start, format!("duplicate attribute {k}")));
            }
            let prefix = if p.is_empty() {
                None
            } else {
                Some(self.intern(p))
            };
            attrs.push(Attr {
                ns: ans,
                prefix,
                local: Box::from(l),
                value: value.into_boxed_str(),
            });
        }
        let parent = self.open.last().copied();
        let idx = self.elems.len();
        let prefix = if prefix.is_empty() {
            None
        } else {
            Some(self.intern(prefix))
        };
        self.elems.push(Elem {
            ns,
            prefix,
            local: Box::from(local),
            attrs,
            decls,
            children: Vec::new(),
            parent,
            start,
            end: start,
        });
        if let Some(p) = parent.and_then(|p| self.elems.get_mut(p)) {
            p.children.push(Child::Elem(idx));
        }
        Ok(idx)
    }

    fn text(&mut self, s: &str, offset: usize) -> Result<(), XmlError> {
        if s.is_empty() {
            return Ok(());
        }
        if !s.chars().all(is_xml_char) {
            return Err(XmlError::InvalidCharacter);
        }
        match self.open.last().and_then(|&i| self.elems.get_mut(i)) {
            Some(e) => {
                if let Some(Child::Text(t)) = e.children.last_mut() {
                    let mut joined = String::with_capacity(t.len().saturating_add(s.len()));
                    joined.push_str(t);
                    joined.push_str(s);
                    *t = joined.into_boxed_str();
                } else {
                    e.children.push(Child::Text(Box::from(s)));
                }
                Ok(())
            }
            None if s.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) => Ok(()),
            None => Err(syntax(offset, "character data outside the root element")),
        }
    }

    fn misc(&mut self, m: Misc) {
        match self.open.last().and_then(|&i| self.elems.get_mut(i)) {
            Some(e) => e.children.push(match m {
                Misc::Comment(a, b) => Child::Comment(a, b),
                Misc::Pi(a, b) => Child::Pi(a, b),
            }),
            None => self.misc.push(m),
        }
    }
}

/// Parses `bytes` into a [`Dom`].
///
/// # Errors
///
/// [`XmlError`] for anything that is not a well-formed, namespace-valid,
/// DTD-free UTF-8 XML document within the limits.
pub(crate) fn parse(bytes: &[u8], limits: XmlLimits) -> Result<Dom<'_>, XmlError> {
    if bytes.len() > limits.max_bytes {
        return Err(XmlError::TooLarge(limits.max_bytes));
    }
    let input = std::str::from_utf8(bytes).map_err(|_| XmlError::NotUtf8)?;
    // quick-xml skips a BOM without counting it in `buffer_position`: strip
    // it here so spans index what the reader saw.
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    if input.starts_with('\u{feff}') {
        return Err(syntax(0, "more than one byte-order mark"));
    }
    let mut reader = quick_xml::Reader::from_reader(input.as_bytes());
    let cfg = reader.config_mut();
    cfg.check_end_names = true;
    cfg.expand_empty_elements = false;
    cfg.trim_text(false);
    let mut b = Builder {
        limits,
        elems: Vec::new(),
        open: Vec::new(),
        misc: Vec::new(),
        interned: HashMap::new(),
        empty: Arc::from(""),
        root_closed: false,
    };
    loop {
        let before = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
        let ev = reader.read_event().map_err(|e| {
            syntax(
                usize::try_from(reader.error_position()).unwrap_or(before),
                e,
            )
        })?;
        let after = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
        match ev {
            Event::Start(e) => {
                let i = b.start(&e, before)?;
                b.open.push(i);
            }
            Event::Empty(e) => {
                let i = b.start(&e, before)?;
                if let Some(el) = b.elems.get_mut(i) {
                    el.end = after;
                }
                if b.open.is_empty() {
                    b.root_closed = true;
                }
            }
            Event::End(_) => {
                let Some(i) = b.open.pop() else {
                    return Err(syntax(before, "end tag with no start tag"));
                };
                if let Some(el) = b.elems.get_mut(i) {
                    el.end = after;
                }
                if b.open.is_empty() {
                    b.root_closed = true;
                }
            }
            Event::Text(t) => {
                let s = t.xml10_content().map_err(|_| XmlError::NotUtf8)?;
                b.text(&s, before)?;
            }
            Event::CData(t) => {
                let s = t.xml10_content().map_err(|_| XmlError::NotUtf8)?;
                b.text(&s, before)?;
            }
            Event::GeneralRef(r) => {
                let c = if r.is_char_ref() {
                    match r.resolve_char_ref() {
                        Ok(Some(c)) if is_xml_char(c) => c,
                        _ => return Err(XmlError::InvalidCharacter),
                    }
                } else {
                    match std::str::from_utf8(&r).map_err(|_| XmlError::NotUtf8)? {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        other => return Err(XmlError::UndefinedEntity(other.to_owned())),
                    }
                };
                let mut buf = [0u8; 4];
                b.text(c.encode_utf8(&mut buf), before)?;
            }
            Event::DocType(_) => return Err(XmlError::Dtd),
            Event::Decl(d) => {
                if let Some(Ok(enc)) = d.encoding()
                    && !enc.eq_ignore_ascii_case(b"utf-8")
                    && !enc.eq_ignore_ascii_case(b"utf8")
                {
                    return Err(XmlError::NotUtf8);
                }
            }
            Event::Comment(c) => {
                if !std::str::from_utf8(&c).is_ok_and(|s| s.chars().all(is_xml_char)) {
                    return Err(XmlError::InvalidCharacter);
                }
                b.misc(Misc::Comment(before, after));
            }
            Event::PI(p) => {
                if !std::str::from_utf8(p.content()).is_ok_and(|s| s.chars().all(is_xml_char)) {
                    return Err(XmlError::InvalidCharacter);
                }
                b.misc(Misc::Pi(before, after));
            }
            Event::Eof => break,
        }
    }
    if !b.open.is_empty() || b.elems.is_empty() {
        return Err(XmlError::NotADocument);
    }
    Ok(Dom {
        input,
        elems: b.elems,
        misc: b.misc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: XmlLimits = XmlLimits {
        max_depth: 16,
        max_elements: 100,
        max_bytes: 1 << 20,
    };

    #[test]
    fn spans_and_namespaces() {
        let s = "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:a=\"urn:a\"><!-- c --><a:x  q = 'v&amp;'><a:y/></a:x><g/></svg>";
        let d = parse(s.as_bytes(), L).unwrap();
        let root = d.root().unwrap();
        assert!(root.is(crate::svg::NS_SVG, "svg"));
        let x = 1;
        let e = d.elem(x).unwrap();
        assert_eq!(&*e.ns, "urn:a");
        assert_eq!(e.get("", "q"), Some("v&"));
        assert_eq!(
            d.fragment(x).unwrap(),
            "<a:x xmlns:a=\"urn:a\"  q = 'v&amp;'><a:y/></a:x>"
        );
        assert_eq!(d.fragment(3).unwrap(), "<g/>");
        assert!(matches!(root.children.first(), Some(Child::Comment(..))));
    }

    #[test]
    fn hostile_input_is_refused() {
        let bad: &[&str] = &[
            "<!DOCTYPE svg [<!ENTITY a 'b'>]><svg/>",
            "<svg>&a;</svg>",
            "<svg><p:x/></svg>",
            "<svg a=\"1\" a=\"2\"/>",
            "<svg/><svg/>",
            "<svg>",
            "text<svg/>",
            "<svg><x0y=\"1\"/></svg>",
        ];
        for s in bad {
            assert!(parse(s.as_bytes(), L).is_err(), "{s}");
        }
        let deep = format!("{}{}", "<g>".repeat(40), "</g>".repeat(40));
        assert!(matches!(
            parse(deep.as_bytes(), L),
            Err(XmlError::TooDeep(16))
        ));
    }
}
