//! `META-INF/manifest.xml` (`research/06 §3.4`, schema in `§11.1`).
//!
//! The manifest is the normative inventory of the package: one
//! `<mf:file-entry>` per ZIP entry, with its role, uncompressed size, method
//! and BLAKE3-256 digest.
//!
//! # Parsing
//!
//! Namespaces are resolved here rather than by `quick-xml`'s `NsReader`,
//! because foreign fragments have to be re-emitted with the declarations
//! they depend on, and that needs the full in-scope binding set, which the
//! `NsReader` does not expose. DTDs are refused, entity references other than
//! the five predefined ones and character references are refused, nesting is
//! capped at [`Limits::max_xml_depth`], and a prefix that is not bound is an
//! error.
//!
//! # Unknown data
//!
//! Everything the schema marks as open (`AnyForeignAttr`, `AnyForeign`) is
//! kept: foreign attributes as `(namespace, local name, value)` and foreign
//! child elements as their **verbatim bytes**, made namespace-complete by
//! inserting the in-scope declarations they inherit into their start tag
//! (`research/06 §8.2` point 2). Unknown attributes in the `mf:` namespace
//! itself — a newer minor version's additions — are kept the same way.
//! Unknown roles are kept verbatim as [`Role::Other`] and treated as
//! `unknown` (§3.4 rule 3).

#![deny(clippy::arithmetic_side_effects)]

use std::collections::{BTreeMap, BTreeSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::PrefixDeclaration;
use thiserror::Error;

use crate::digest::{BLAKE3_256, Digest};
use crate::limits::Limits;
use crate::name::validate_name;
use crate::{FORMAT_VERSION, MIME_TYPE, Method, Profile, Version};

/// The manifest namespace.
pub const MANIFEST_NS: &str = "https://xarast.org/ns/manifest/1.0";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

/// Why a manifest could not be parsed or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ManifestError {
    /// Not well-formed XML.
    #[error("malformed XML: {0}")]
    Xml(String),
    /// Not UTF-8.
    #[error("not UTF-8")]
    NotUtf8,
    /// A DOCTYPE was present. DTDs are refused (`research/06 §10.5`).
    #[error("DTDs are not allowed")]
    Dtd,
    /// An entity reference other than the five predefined ones.
    #[error("undefined entity reference &{0};")]
    UndefinedEntity(String),
    /// A character XML 1.0 does not allow.
    #[error("character not allowed in XML")]
    InvalidCharacter,
    /// Nesting deeper than [`Limits::max_xml_depth`].
    #[error("XML nested too deeply")]
    TooDeep,
    /// A prefix with no namespace binding.
    #[error("unbound namespace prefix {0:?}")]
    UnboundPrefix(String),
    /// The root element is not `mf:manifest`, or there is more than one.
    #[error("the root element is not a single mf:manifest")]
    NotAManifest,
    /// A mandatory attribute is absent.
    #[error("<{element}> lacks the mandatory attribute {attribute}")]
    MissingAttribute {
        /// Element local name.
        element: &'static str,
        /// Attribute local name.
        attribute: &'static str,
    },
    /// An attribute value does not match the schema.
    #[error("invalid value {value:?} for {attribute}")]
    BadValue {
        /// Attribute local name.
        attribute: &'static str,
        /// The offending value.
        value: String,
    },
}

/// The role of an entry (`research/06 §3.4` rule 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    /// `mimetype`.
    Mimetype,
    /// `manifest`.
    Manifest,
    /// `meta`.
    Meta,
    /// `document`.
    Document,
    /// `thumbnail`.
    Thumbnail,
    /// `preview`.
    Preview,
    /// `resource`.
    Resource,
    /// `history`.
    History,
    /// `extension`.
    Extension,
    /// `unknown`.
    Unknown,
    /// A role this version does not know. Treated as [`Role::Unknown`], and
    /// written back verbatim.
    Other(String),
}

impl Role {
    /// Parses a role; an unrecognised one becomes [`Role::Other`].
    pub fn parse(s: &str) -> Role {
        match s {
            "mimetype" => Role::Mimetype,
            "manifest" => Role::Manifest,
            "meta" => Role::Meta,
            "document" => Role::Document,
            "thumbnail" => Role::Thumbnail,
            "preview" => Role::Preview,
            "resource" => Role::Resource,
            "history" => Role::History,
            "extension" => Role::Extension,
            "unknown" => Role::Unknown,
            other => Role::Other(other.to_owned()),
        }
    }

    /// The manifest spelling.
    pub fn as_str(&self) -> &str {
        match self {
            Role::Mimetype => "mimetype",
            Role::Manifest => "manifest",
            Role::Meta => "meta",
            Role::Document => "document",
            Role::Thumbnail => "thumbnail",
            Role::Preview => "preview",
            Role::Resource => "resource",
            Role::History => "history",
            Role::Extension => "extension",
            Role::Unknown => "unknown",
            Role::Other(s) => s,
        }
    }

    /// Whether the role must carry a digest (§3.4 rule 4).
    pub fn requires_digest(&self) -> bool {
        matches!(self, Role::Document | Role::Meta | Role::Resource)
    }
}

/// A digest as the manifest records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryDigest {
    /// `blake3-256`, the only algorithm v1.0 defines.
    Blake3(Digest),
    /// An algorithm from a later version: carried, never verified.
    Other {
        /// `mf:digest`.
        algorithm: String,
        /// `mf:digest-value`.
        value: String,
    },
}

/// An attribute this version does not understand, carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ForeignAttr {
    /// Namespace URI; empty for an unqualified attribute.
    pub namespace: String,
    /// Local name.
    pub local: String,
    /// The prefix it was read with, used as a hint when writing.
    pub prefix: Option<String>,
    /// The attribute value, unescaped.
    pub value: String,
}

/// A foreign child element, as the exact bytes that were read, with the
/// namespace declarations it inherited inserted into its start tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ForeignElement {
    /// The fragment, well-formed and namespace-complete on its own.
    pub xml: String,
}

/// A `<mf:capability>` (`research/06 §7.4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// `mf:name`.
    pub name: String,
    /// `mf:optional`.
    pub optional: bool,
    /// Attributes this version does not understand.
    pub foreign_attrs: Vec<ForeignAttr>,
    /// Child elements this version does not understand.
    pub foreign_children: Vec<ForeignElement>,
}

/// One `<mf:file-entry>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// `mf:full-path`.
    pub full_path: String,
    /// `mf:media-type`.
    pub media_type: String,
    /// `mf:role`.
    pub role: Option<Role>,
    /// `mf:size`: the uncompressed size.
    pub size: Option<u64>,
    /// `mf:method`: informative, the ZIP is the source of truth. An
    /// unrecognised value is dropped, since the writer recomputes it.
    pub method: Option<Method>,
    /// `mf:digest` + `mf:digest-value`.
    pub digest: Option<EntryDigest>,
    /// `mf:refcount`: informative.
    pub refcount: Option<u64>,
    /// `mf:derived-from`: the master of a derived rendition (§4.4).
    pub derived_from: Option<Digest>,
    /// `mf:derivation`: the operation chain that produced it.
    pub derivation: Option<String>,
    /// Attributes this version does not understand.
    pub foreign_attrs: Vec<ForeignAttr>,
    /// Child elements this version does not understand.
    pub foreign_children: Vec<ForeignElement>,
}

impl FileEntry {
    /// A row with only the mandatory fields.
    pub fn new(full_path: impl Into<String>, media_type: impl Into<String>) -> FileEntry {
        FileEntry {
            full_path: full_path.into(),
            media_type: media_type.into(),
            role: None,
            size: None,
            method: None,
            digest: None,
            refcount: None,
            derived_from: None,
            derivation: None,
            foreign_attrs: Vec::new(),
            foreign_children: Vec::new(),
        }
    }

    /// The BLAKE3-256 digest, when the row has one.
    pub fn blake3(&self) -> Option<Digest> {
        match &self.digest {
            Some(EntryDigest::Blake3(d)) => Some(*d),
            _ => None,
        }
    }

    /// The role, with an absent or unrecognised role read as `unknown`.
    pub fn effective_role(&self) -> Role {
        match &self.role {
            None | Some(Role::Other(_)) => Role::Unknown,
            Some(r) => r.clone(),
        }
    }
}

/// The parsed manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// `mf:version`.
    pub version: Version,
    /// `mf:min-reader`.
    pub min_reader: Version,
    /// `mf:generator`.
    pub generator: Option<String>,
    /// `mf:profile`.
    pub profile: Profile,
    /// `<mf:requires>`.
    pub requires: Vec<Capability>,
    /// Unknown children of `<mf:requires>`.
    pub requires_foreign: Vec<ForeignElement>,
    /// The media type of the `/` row (§3.4 rule 1), if there is one.
    pub root_media_type: Option<String>,
    /// How many `/` rows were read. Exactly one is conformant.
    pub root_rows: u32,
    /// Unknown attributes of the `/` row.
    pub root_foreign_attrs: Vec<ForeignAttr>,
    /// Unknown children of the `/` row (of every `/` row, if there were
    /// several). Kept inside the row: moved to the manifest level, a nested
    /// `mf:file-entry` would turn into a real row.
    pub root_foreign_children: Vec<ForeignElement>,
    /// Every row except `/`, in document order.
    pub entries: Vec<FileEntry>,
    /// Unknown attributes of `<mf:manifest>`.
    pub foreign_attrs: Vec<ForeignAttr>,
    /// Unknown children of `<mf:manifest>`.
    pub foreign_children: Vec<ForeignElement>,
}

impl Manifest {
    /// An empty manifest for this format version.
    pub fn new(profile: Profile, generator: Option<String>) -> Manifest {
        Manifest {
            version: FORMAT_VERSION,
            min_reader: FORMAT_VERSION,
            generator,
            profile,
            requires: Vec::new(),
            requires_foreign: Vec::new(),
            root_media_type: Some(MIME_TYPE.to_owned()),
            root_rows: 1,
            root_foreign_attrs: Vec::new(),
            root_foreign_children: Vec::new(),
            entries: Vec::new(),
            foreign_attrs: Vec::new(),
            foreign_children: Vec::new(),
        }
    }

    /// The first row with this path.
    pub fn entry(&self, path: &str) -> Option<&FileEntry> {
        self.entries.iter().find(|e| e.full_path == path)
    }

    /// Parses a manifest.
    pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Manifest, ManifestError> {
        Parser::new(bytes, limits)?.run()
    }

    /// Serialises the manifest. Attribute order, prefixes and indentation are
    /// fixed, so the same manifest always produces the same bytes (O8).
    pub fn to_xml(&self) -> Result<String, ManifestError> {
        write_manifest(self)
    }
}

// ─────────────────────────────────────────────────────────────── parsing

/// A start tag with its names resolved.
struct Element {
    ns: Option<String>,
    local: String,
    attrs: Vec<ResolvedAttr>,
    /// Prefixes the tag uses (`""` for an unprefixed element name).
    used: Vec<String>,
}

struct ResolvedAttr {
    ns: Option<String>,
    prefix: Option<String>,
    local: String,
    value: String,
}

impl Element {
    fn is_mf(&self, local: &str) -> bool {
        self.ns.as_deref() == Some(MANIFEST_NS) && self.local == local
    }
}

#[derive(Clone, Copy)]
enum Ctx {
    Manifest,
    Entry(usize),
    RootRow,
    Requires,
    Capability(usize),
}

/// Where a captured foreign fragment goes: always the element it was read
/// inside, so that re-emitting it can never change what it means.
#[derive(Clone, Copy)]
enum Dest {
    Manifest,
    Entry(usize),
    RootRow,
    Requires,
    Capability(usize),
}

struct Capture {
    start: usize,
    depth: u32,
    dest: Dest,
    inherited: BTreeMap<String, String>,
    used: BTreeSet<String>,
}

struct Parser<'a> {
    input: &'a str,
    reader: quick_xml::Reader<&'a [u8]>,
    limits: Limits,
    /// Namespace bindings per open element; `""` is the default namespace.
    scopes: Vec<Vec<(String, String)>>,
    ctx: Vec<Ctx>,
    capture: Option<Capture>,
    manifest: Option<Manifest>,
    seen_root: bool,
}

fn xml_err(e: impl std::fmt::Display) -> ManifestError {
    ManifestError::Xml(e.to_string())
}

fn utf8(b: &[u8]) -> Result<&str, ManifestError> {
    std::str::from_utf8(b).map_err(|_| ManifestError::NotUtf8)
}

/// XML 1.0 `Char`, minus the surrogates Rust cannot hold anyway.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn check_chars(s: &str) -> Result<(), ManifestError> {
    if s.chars().all(is_xml_char) {
        Ok(())
    } else {
        Err(ManifestError::InvalidCharacter)
    }
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8], limits: &Limits) -> Result<Self, ManifestError> {
        let input = utf8(bytes)?;
        // quick-xml skips a BOM without counting it in `buffer_position`, so
        // the fragment offsets would be three bytes short. Strip it here and
        // slice fragments from what the reader actually sees.
        let input = input.strip_prefix('\u{feff}').unwrap_or(input);
        // quick-xml would skip a second BOM too, again without counting
        // it. A second one is character data before the prolog anyway.
        if input.starts_with('\u{feff}') {
            return Err(xml_err("more than one byte-order mark"));
        }
        check_chars(input)?;
        let mut reader = quick_xml::Reader::from_reader(input.as_bytes());
        let cfg = reader.config_mut();
        cfg.check_end_names = true;
        cfg.expand_empty_elements = false;
        cfg.trim_text(false);
        Ok(Parser {
            input,
            reader,
            limits: *limits,
            scopes: Vec::new(),
            ctx: Vec::new(),
            capture: None,
            manifest: None,
            seen_root: false,
        })
    }

    fn resolve(&self, prefix: &str, attr: bool) -> Result<Option<String>, ManifestError> {
        if prefix == "xml" {
            return Ok(Some(XML_NS.to_owned()));
        }
        if attr && prefix.is_empty() {
            return Ok(None);
        }
        for scope in self.scopes.iter().rev() {
            if let Some((_, uri)) = scope.iter().rev().find(|(p, _)| p == prefix) {
                return Ok((!uri.is_empty()).then(|| uri.clone()));
            }
        }
        if prefix.is_empty() {
            Ok(None)
        } else {
            Err(ManifestError::UnboundPrefix(prefix.to_owned()))
        }
    }

    /// Pushes the element's namespace scope and resolves its names.
    fn open(&mut self, e: &BytesStart<'_>) -> Result<Element, ManifestError> {
        let mut scope = Vec::new();
        let mut raw_attrs = Vec::new();
        for a in e.attributes().with_checks(true) {
            let a = a.map_err(xml_err)?;
            let value = a
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|err| match err {
                    quick_xml::Error::Escape(
                        quick_xml::escape::EscapeError::UnrecognizedEntity(_, name),
                    ) => ManifestError::UndefinedEntity(name),
                    other => xml_err(other),
                })?
                .into_owned();
            check_chars(&value)?;
            match a.key.as_namespace_binding() {
                Some(PrefixDeclaration::Default) => scope.push((String::new(), value)),
                Some(PrefixDeclaration::Named(p)) => {
                    let p = utf8(p)?.to_owned();
                    if !is_ncname(&p) {
                        return Err(xml_err(format!("invalid prefix {p:?}")));
                    }
                    if p == "xmlns"
                        || (p == "xml" && value != XML_NS)
                        || (p != "xml" && value == XML_NS)
                    {
                        return Err(ManifestError::BadValue {
                            attribute: "xmlns",
                            value,
                        });
                    }
                    if value.is_empty() {
                        // Prefix undeclaration is XML 1.1 only.
                        return Err(ManifestError::BadValue {
                            attribute: "xmlns",
                            value,
                        });
                    }
                    scope.push((p, value));
                }
                None => raw_attrs.push((utf8(a.key.as_ref())?.to_owned(), value)),
            }
        }
        self.scopes.push(scope);
        let qname = utf8(e.name().as_ref())?.to_owned();
        let (prefix, local) = split_qname(&qname)?;
        let ns = self.resolve(prefix, false)?;
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        let mut used = vec![prefix.to_owned()];
        let mut seen = BTreeSet::new();
        for (key, value) in raw_attrs {
            let (p, l) = split_qname(&key)?;
            let ans = self.resolve(p, true)?;
            // Two prefixes for one namespace with the same local name is a
            // duplicate attribute after resolution.
            if !seen.insert((ans.clone(), l.to_owned())) {
                return Err(xml_err(format!("duplicate attribute {key}")));
            }
            if !p.is_empty() {
                used.push(p.to_owned());
            }
            attrs.push(ResolvedAttr {
                ns: ans,
                prefix: (!p.is_empty()).then(|| p.to_owned()),
                local: l.to_owned(),
                value,
            });
        }
        Ok(Element {
            ns,
            local: local.to_owned(),
            attrs,
            used,
        })
    }

    fn inherited_bindings(&self) -> BTreeMap<String, String> {
        // Everything in scope for the fragment root except what it declares
        // itself (the innermost scope, which is the root's own).
        let mut map = BTreeMap::new();
        let n = self.scopes.len().saturating_sub(1);
        for scope in self.scopes.iter().take(n) {
            for (p, u) in scope {
                map.insert(p.clone(), u.clone());
            }
        }
        if let Some(own) = self.scopes.last() {
            for (p, _) in own {
                map.remove(p);
            }
        }
        map.retain(|p, u| !(p.is_empty() && u.is_empty()));
        map
    }

    fn run(mut self) -> Result<Manifest, ManifestError> {
        let mut depth: u32 = 0;
        loop {
            let before = usize::try_from(self.reader.buffer_position()).map_err(xml_err)?;
            let ev = self.reader.read_event().map_err(xml_err)?;
            match ev {
                Event::Start(e) => {
                    depth = depth.checked_add(1).ok_or(ManifestError::TooDeep)?;
                    if depth > self.limits.max_xml_depth {
                        return Err(ManifestError::TooDeep);
                    }
                    let el = self.open(&e)?;
                    self.start(el, before, false)?;
                }
                Event::Empty(e) => {
                    if depth >= self.limits.max_xml_depth {
                        return Err(ManifestError::TooDeep);
                    }
                    let el = self.open(&e)?;
                    self.start(el, before, true)?;
                    self.scopes.pop();
                }
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    self.scopes.pop();
                    self.end()?;
                }
                Event::GeneralRef(r) => {
                    let ok = if r.is_char_ref() {
                        matches!(r.resolve_char_ref(), Ok(Some(c)) if is_xml_char(c))
                    } else {
                        matches!(utf8(&r)?, "lt" | "gt" | "amp" | "apos" | "quot")
                    };
                    if !ok {
                        return Err(ManifestError::UndefinedEntity(utf8(&r)?.to_owned()));
                    }
                    self.text_outside_root(depth)?;
                }
                Event::Text(t) => {
                    if depth == 0 && !t.iter().all(u8::is_ascii_whitespace) {
                        return Err(ManifestError::NotAManifest);
                    }
                }
                Event::CData(_) => self.text_outside_root(depth)?,
                Event::DocType(_) => return Err(ManifestError::Dtd),
                Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
                Event::Eof => break,
            }
        }
        if depth != 0 || self.capture.is_some() {
            return Err(xml_err("unexpected end of input"));
        }
        self.manifest.ok_or(ManifestError::NotAManifest)
    }

    fn text_outside_root(&self, depth: u32) -> Result<(), ManifestError> {
        if depth == 0 {
            Err(ManifestError::NotAManifest)
        } else {
            Ok(())
        }
    }

    fn start(&mut self, el: Element, before: usize, empty: bool) -> Result<(), ManifestError> {
        if let Some(cap) = self.capture.as_mut() {
            if !empty {
                cap.depth = cap.depth.saturating_add(1);
            }
            cap.used.extend(el.used);
            return Ok(());
        }
        let Some(&top) = self.ctx.last() else {
            if self.seen_root || !el.is_mf("manifest") {
                return Err(ManifestError::NotAManifest);
            }
            self.seen_root = true;
            self.manifest = Some(parse_root(el)?);
            if !empty {
                self.ctx.push(Ctx::Manifest);
            }
            return Ok(());
        };
        let m = self.manifest.as_mut().ok_or(ManifestError::NotAManifest)?;
        let dest = match top {
            Ctx::Manifest if el.is_mf("file-entry") => {
                let entry = parse_entry(el)?;
                if entry.full_path == "/" {
                    m.root_rows = m.root_rows.saturating_add(1);
                    if m.root_media_type.is_none() {
                        m.root_media_type = Some(entry.media_type.clone());
                        m.root_foreign_attrs = entry.foreign_attrs;
                    }
                    if !empty {
                        self.ctx.push(Ctx::RootRow);
                    }
                } else {
                    m.entries.push(entry);
                    if !empty {
                        self.ctx.push(Ctx::Entry(m.entries.len().saturating_sub(1)));
                    }
                }
                return Ok(());
            }
            Ctx::Manifest if el.is_mf("requires") => {
                if !empty {
                    self.ctx.push(Ctx::Requires);
                }
                return Ok(());
            }
            Ctx::Requires if el.is_mf("capability") => {
                m.requires.push(parse_capability(el)?);
                if !empty {
                    self.ctx
                        .push(Ctx::Capability(m.requires.len().saturating_sub(1)));
                }
                return Ok(());
            }
            Ctx::Manifest => Dest::Manifest,
            Ctx::Entry(i) => Dest::Entry(i),
            Ctx::RootRow => Dest::RootRow,
            Ctx::Requires => Dest::Requires,
            Ctx::Capability(i) => Dest::Capability(i),
        };
        let inherited = self.inherited_bindings();
        let used: BTreeSet<String> = el.used.into_iter().collect();
        if empty {
            let after = usize::try_from(self.reader.buffer_position()).map_err(xml_err)?;
            self.store(before, after, dest, &inherited, &used)?;
        } else {
            self.capture = Some(Capture {
                start: before,
                depth: 1,
                dest,
                inherited,
                used,
            });
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), ManifestError> {
        if let Some(cap) = self.capture.as_mut() {
            cap.depth = cap.depth.saturating_sub(1);
            if cap.depth == 0 {
                let after = usize::try_from(self.reader.buffer_position()).map_err(xml_err)?;
                if let Some(cap) = self.capture.take() {
                    self.store(cap.start, after, cap.dest, &cap.inherited, &cap.used)?;
                }
            }
            return Ok(());
        }
        self.ctx.pop();
        Ok(())
    }

    fn store(
        &mut self,
        start: usize,
        end: usize,
        dest: Dest,
        inherited: &BTreeMap<String, String>,
        used: &BTreeSet<String>,
    ) -> Result<(), ManifestError> {
        let raw = self
            .input
            .get(start..end)
            .ok_or_else(|| xml_err("fragment out of range"))?;
        // Only the bindings the fragment actually uses: declaring every
        // binding in scope would make the second save differ from the first.
        let needed: BTreeMap<String, String> = inherited
            .iter()
            .filter(|(p, _)| used.contains(*p))
            .map(|(p, u)| (p.clone(), u.clone()))
            .collect();
        let el = ForeignElement {
            xml: complete_fragment(raw, &needed),
        };
        let m = self.manifest.as_mut().ok_or(ManifestError::NotAManifest)?;
        match dest {
            Dest::Manifest => m.foreign_children.push(el),
            Dest::Requires => m.requires_foreign.push(el),
            Dest::RootRow => m.root_foreign_children.push(el),
            Dest::Entry(i) => {
                if let Some(e) = m.entries.get_mut(i) {
                    e.foreign_children.push(el);
                }
            }
            Dest::Capability(i) => {
                if let Some(c) = m.requires.get_mut(i) {
                    c.foreign_children.push(el);
                }
            }
        }
        Ok(())
    }
}

/// Splits and validates a qualified name. `quick-xml` does not check names
/// (it will take `a="b"` as an element name if nothing separates it from the
/// tag), so every name is checked here before it is trusted.
fn split_qname(q: &str) -> Result<(&str, &str), ManifestError> {
    let (prefix, local) = q.split_once(':').unwrap_or(("", q));
    let prefix_ok = if q.contains(':') {
        is_ncname(prefix)
    } else {
        true
    };
    if prefix_ok && is_ncname(local) {
        Ok((prefix, local))
    } else {
        Err(xml_err(format!("invalid name {q:?}")))
    }
}

/// Inserts the inherited namespace declarations right after the element name
/// of the fragment's start tag, so the fragment stands on its own.
fn complete_fragment(raw: &str, inherited: &BTreeMap<String, String>) -> String {
    if inherited.is_empty() {
        return raw.to_owned();
    }
    let name_end = raw
        .char_indices()
        .skip(1)
        .find(|(_, c)| c.is_ascii_whitespace() || *c == '/' || *c == '>')
        .map_or(raw.len(), |(i, _)| i);
    let (head, tail) = raw.split_at(name_end);
    let mut out = String::with_capacity(raw.len().saturating_add(64));
    out.push_str(head);
    for (p, u) in inherited {
        if p.is_empty() {
            out.push_str(" xmlns=\"");
        } else {
            out.push_str(" xmlns:");
            out.push_str(p);
            out.push_str("=\"");
        }
        escape_attr_into(&mut out, u);
        out.push('"');
    }
    out.push_str(tail);
    out
}

fn foreign(a: ResolvedAttr) -> ForeignAttr {
    ForeignAttr {
        namespace: a.ns.unwrap_or_default(),
        local: a.local,
        prefix: a.prefix,
        value: a.value,
    }
}

fn is_mf_attr(a: &ResolvedAttr) -> bool {
    a.ns.as_deref() == Some(MANIFEST_NS)
}

fn parse_u64(attribute: &'static str, v: &str) -> Result<u64, ManifestError> {
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ManifestError::BadValue {
            attribute,
            value: v.to_owned(),
        });
    }
    v.parse().map_err(|_| ManifestError::BadValue {
        attribute,
        value: v.to_owned(),
    })
}

fn parse_version(attribute: &'static str, v: &str) -> Result<Version, ManifestError> {
    Version::parse(v).ok_or_else(|| ManifestError::BadValue {
        attribute,
        value: v.to_owned(),
    })
}

fn parse_root(el: Element) -> Result<Manifest, ManifestError> {
    let mut version = None;
    let mut min_reader = None;
    let mut generator = None;
    let mut profile = None;
    let mut foreign_attrs = Vec::new();
    for a in el.attrs {
        if !is_mf_attr(&a) {
            foreign_attrs.push(foreign(a));
            continue;
        }
        match a.local.as_str() {
            "version" => version = Some(parse_version("version", &a.value)?),
            "min-reader" => min_reader = Some(parse_version("min-reader", &a.value)?),
            "generator" => generator = Some(a.value),
            "profile" => {
                profile = Some(
                    Profile::parse(&a.value).ok_or_else(|| ManifestError::BadValue {
                        attribute: "profile",
                        value: a.value.clone(),
                    })?,
                )
            }
            _ => foreign_attrs.push(foreign(a)),
        }
    }
    let missing = |attribute| ManifestError::MissingAttribute {
        element: "manifest",
        attribute,
    };
    Ok(Manifest {
        version: version.ok_or_else(|| missing("version"))?,
        min_reader: min_reader.ok_or_else(|| missing("min-reader"))?,
        generator,
        profile: profile.ok_or_else(|| missing("profile"))?,
        requires: Vec::new(),
        requires_foreign: Vec::new(),
        root_media_type: None,
        root_rows: 0,
        root_foreign_attrs: Vec::new(),
        root_foreign_children: Vec::new(),
        entries: Vec::new(),
        foreign_attrs,
        foreign_children: Vec::new(),
    })
}

fn parse_entry(el: Element) -> Result<FileEntry, ManifestError> {
    let mut full_path = None;
    let mut media_type = None;
    let mut e = FileEntry::new(String::new(), String::new());
    let mut digest_alg = None;
    let mut digest_value = None;
    for a in el.attrs {
        if !is_mf_attr(&a) {
            e.foreign_attrs.push(foreign(a));
            continue;
        }
        match a.local.as_str() {
            "full-path" => full_path = Some(a.value),
            "media-type" => media_type = Some(a.value),
            "role" => e.role = Some(Role::parse(&a.value)),
            "size" => e.size = Some(parse_u64("size", &a.value)?),
            "method" => e.method = Method::parse(&a.value),
            "digest" => digest_alg = Some(a.value),
            "digest-value" => digest_value = Some(a.value),
            "refcount" => e.refcount = Some(parse_u64("refcount", &a.value)?),
            "derived-from" => {
                e.derived_from =
                    Some(
                        Digest::from_hex(&a.value).ok_or_else(|| ManifestError::BadValue {
                            attribute: "derived-from",
                            value: a.value.clone(),
                        })?,
                    )
            }
            "derivation" => e.derivation = Some(a.value),
            _ => e.foreign_attrs.push(foreign(a)),
        }
    }
    let missing = |attribute| ManifestError::MissingAttribute {
        element: "file-entry",
        attribute,
    };
    let full_path = full_path.ok_or_else(|| missing("full-path"))?;
    if full_path != "/" && validate_name(&full_path).is_err() {
        return Err(ManifestError::BadValue {
            attribute: "full-path",
            value: full_path,
        });
    }
    e.full_path = full_path;
    e.media_type = media_type.ok_or_else(|| missing("media-type"))?;
    e.digest = match (digest_alg, digest_value) {
        (None, None) => None,
        (Some(alg), Some(value)) if alg == BLAKE3_256 => Some(EntryDigest::Blake3(
            Digest::from_hex(&value).ok_or(ManifestError::BadValue {
                attribute: "digest-value",
                value,
            })?,
        )),
        (Some(algorithm), Some(value)) => Some(EntryDigest::Other { algorithm, value }),
        (None, Some(_)) => return Err(missing("digest")),
        (Some(_), None) => return Err(missing("digest-value")),
    };
    Ok(e)
}

fn parse_capability(el: Element) -> Result<Capability, ManifestError> {
    let mut name = None;
    let mut optional = false;
    let mut foreign_attrs = Vec::new();
    for a in el.attrs {
        if !is_mf_attr(&a) {
            foreign_attrs.push(foreign(a));
            continue;
        }
        match a.local.as_str() {
            "name" => name = Some(a.value),
            "optional" => {
                optional = match a.value.trim() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => {
                        return Err(ManifestError::BadValue {
                            attribute: "optional",
                            value: a.value,
                        });
                    }
                }
            }
            _ => foreign_attrs.push(foreign(a)),
        }
    }
    Ok(Capability {
        name: name.ok_or(ManifestError::MissingAttribute {
            element: "capability",
            attribute: "name",
        })?,
        optional,
        foreign_attrs,
        foreign_children: Vec::new(),
    })
}

// ─────────────────────────────────────────────────────────────── writing

fn escape_attr_into(out: &mut String, v: &str) {
    for c in v.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Literal whitespace other than space is normalised to a space by
            // every reader; character references survive.
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

fn is_ncname(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && cs.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Assigns a prefix to every foreign namespace, deterministically: the prefix
/// it was read with when that is free, `ns1`, `ns2`, … otherwise.
fn assign_prefixes(m: &Manifest) -> BTreeMap<String, String> {
    let mut wanted: BTreeMap<String, Option<String>> = BTreeMap::new();
    let attrs = m
        .foreign_attrs
        .iter()
        .chain(&m.root_foreign_attrs)
        .chain(m.entries.iter().flat_map(|e| &e.foreign_attrs))
        .chain(m.requires.iter().flat_map(|c| &c.foreign_attrs));
    for a in attrs {
        if a.namespace.is_empty() || a.namespace == MANIFEST_NS || a.namespace == XML_NS {
            continue;
        }
        let slot = wanted.entry(a.namespace.clone()).or_default();
        if slot.is_none() {
            slot.clone_from(&a.prefix);
        }
    }
    let mut used: BTreeSet<String> = ["mf", "xml", "xmlns"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut out = BTreeMap::new();
    let mut n: u32 = 0;
    for (ns, hint) in wanted {
        let p = match hint {
            Some(h)
                if is_ncname(&h)
                    && !h.to_ascii_lowercase().starts_with("xml")
                    && !used.contains(&h) =>
            {
                h
            }
            _ => loop {
                n = n.saturating_add(1);
                let cand = format!("ns{n}");
                if !used.contains(&cand) {
                    break cand;
                }
            },
        };
        used.insert(p.clone());
        out.insert(ns, p);
    }
    out
}

struct Out<'p> {
    s: String,
    prefixes: &'p BTreeMap<String, String>,
}

impl Out<'_> {
    fn attr(&mut self, qname: &str, value: &str) -> Result<(), ManifestError> {
        check_chars(value)?;
        self.s.push(' ');
        self.s.push_str(qname);
        self.s.push_str("=\"");
        escape_attr_into(&mut self.s, value);
        self.s.push('"');
        Ok(())
    }

    fn foreign_attrs(&mut self, attrs: &[ForeignAttr]) -> Result<(), ManifestError> {
        let mut sorted: Vec<&ForeignAttr> = attrs.iter().collect();
        sorted.sort_by(|a, b| (&a.namespace, &a.local).cmp(&(&b.namespace, &b.local)));
        for a in sorted {
            if !is_ncname(&a.local) {
                return Err(ManifestError::BadValue {
                    attribute: "foreign attribute name",
                    value: a.local.clone(),
                });
            }
            let prefix = match a.namespace.as_str() {
                "" => None,
                MANIFEST_NS => Some("mf"),
                XML_NS => Some("xml"),
                XMLNS_NS => {
                    return Err(ManifestError::BadValue {
                        attribute: "foreign attribute namespace",
                        value: a.namespace.clone(),
                    });
                }
                ns => Some(self.prefixes.get(ns).map_or("ns", String::as_str)),
            };
            let q = match prefix {
                Some(p) => format!("{p}:{}", a.local),
                None => a.local.clone(),
            };
            self.attr(&q, &a.value)?;
        }
        Ok(())
    }

    fn children(&mut self, kids: &[ForeignElement], indent: &str) {
        for k in kids {
            self.s.push_str(indent);
            self.s.push_str(&k.xml);
            self.s.push('\n');
        }
    }
}

fn write_manifest(m: &Manifest) -> Result<String, ManifestError> {
    let prefixes = assign_prefixes(m);
    let mut o = Out {
        s: String::with_capacity(256usize.saturating_add(m.entries.len().saturating_mul(200))),
        prefixes: &prefixes,
    };
    o.s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mf:manifest");
    o.attr("xmlns:mf", MANIFEST_NS)?;
    for (ns, p) in &prefixes {
        o.attr(&format!("xmlns:{p}"), ns)?;
    }
    o.attr("mf:version", &m.version.to_string())?;
    o.attr("mf:min-reader", &m.min_reader.to_string())?;
    o.attr("mf:profile", m.profile.as_str())?;
    if let Some(g) = &m.generator {
        o.attr("mf:generator", g)?;
    }
    o.foreign_attrs(&m.foreign_attrs)?;
    o.s.push_str(">\n");
    if !m.requires.is_empty() || !m.requires_foreign.is_empty() {
        o.s.push_str("  <mf:requires>\n");
        for c in &m.requires {
            o.s.push_str("    <mf:capability");
            o.attr("mf:name", &c.name)?;
            if c.optional {
                o.attr("mf:optional", "true")?;
            }
            o.foreign_attrs(&c.foreign_attrs)?;
            if c.foreign_children.is_empty() {
                o.s.push_str("/>\n");
            } else {
                o.s.push_str(">\n");
                o.children(&c.foreign_children, "      ");
                o.s.push_str("    </mf:capability>\n");
            }
        }
        o.children(&m.requires_foreign, "    ");
        o.s.push_str("  </mf:requires>\n");
    }
    o.s.push_str("  <mf:file-entry");
    o.attr("mf:full-path", "/")?;
    o.attr(
        "mf:media-type",
        m.root_media_type.as_deref().unwrap_or(MIME_TYPE),
    )?;
    o.foreign_attrs(&m.root_foreign_attrs)?;
    if m.root_foreign_children.is_empty() {
        o.s.push_str("/>\n");
    } else {
        o.s.push_str(">\n");
        o.children(&m.root_foreign_children, "    ");
        o.s.push_str("  </mf:file-entry>\n");
    }
    for e in &m.entries {
        o.s.push_str("  <mf:file-entry");
        o.attr("mf:full-path", &e.full_path)?;
        o.attr("mf:media-type", &e.media_type)?;
        if let Some(r) = &e.role {
            o.attr("mf:role", r.as_str())?;
        }
        if let Some(s) = e.size {
            o.attr("mf:size", &s.to_string())?;
        }
        if let Some(meth) = e.method {
            o.attr("mf:method", meth.as_str())?;
        }
        match &e.digest {
            Some(EntryDigest::Blake3(d)) => {
                o.attr("mf:digest", BLAKE3_256)?;
                o.attr("mf:digest-value", &d.to_hex())?;
            }
            Some(EntryDigest::Other { algorithm, value }) => {
                o.attr("mf:digest", algorithm)?;
                o.attr("mf:digest-value", value)?;
            }
            None => {}
        }
        if let Some(r) = e.refcount {
            o.attr("mf:refcount", &r.to_string())?;
        }
        if let Some(d) = &e.derived_from {
            o.attr("mf:derived-from", &d.to_hex())?;
        }
        if let Some(d) = &e.derivation {
            o.attr("mf:derivation", d)?;
        }
        o.foreign_attrs(&e.foreign_attrs)?;
        if e.foreign_children.is_empty() {
            o.s.push_str("/>\n");
        } else {
            o.s.push_str(">\n");
            o.children(&e.foreign_children, "    ");
            o.s.push_str("  </mf:file-entry>\n");
        }
    }
    o.children(&m.foreign_children, "  ");
    o.s.push_str("</mf:manifest>\n");
    Ok(o.s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The complete example of `research/06 §12.4`, verbatim.
    const SPEC_EXAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0"
             mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"
             mf:generator="Xarast/0.1.0 (linux; x86_64)">
  <mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip"/>
  <mf:file-entry mf:full-path="mimetype" mf:media-type="text/plain"
                 mf:role="mimetype" mf:size="26" mf:method="stored"/>
  <mf:file-entry mf:full-path="META-INF/manifest.xml" mf:media-type="application/xml"
                 mf:role="manifest" mf:method="deflate"/>
  <mf:file-entry mf:full-path="meta.xml" mf:media-type="application/xml"
                 mf:role="meta" mf:size="1841" mf:method="deflate"
                 mf:digest="blake3-256"
                 mf:digest-value="0292ad76bf30d13dce31f284842a8b01f772b36a7974775ae9f2c0eb46a5f956"/>
  <mf:file-entry mf:full-path="document.svg" mf:media-type="image/svg+xml"
                 mf:role="document" mf:size="3375" mf:method="deflate"
                 mf:digest="blake3-256"
                 mf:digest-value="e07d64cd960dc94e50d9030c9d1c3333d8b91c877bc113e535ade8d3ba5adc47"/>
  <mf:file-entry mf:full-path="thumbnail.png" mf:media-type="image/png"
                 mf:role="thumbnail" mf:size="571" mf:method="stored"
                 mf:digest="blake3-256"
                 mf:digest-value="4ab11d84d1b953e36c278f709e317dd045c2242fdddcda28e5f1b35d02910607"/>
</mf:manifest>
"#;

    fn parse(s: &str) -> Result<Manifest, ManifestError> {
        Manifest::parse(s.as_bytes(), &Limits::DEFAULT)
    }

    #[test]
    fn parses_the_spec_example() {
        let m = parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(m.version, Version { major: 1, minor: 0 });
        assert_eq!(m.profile, Profile::Portable);
        assert_eq!(m.generator.as_deref(), Some("Xarast/0.1.0 (linux; x86_64)"));
        assert_eq!(m.root_media_type.as_deref(), Some(MIME_TYPE));
        assert_eq!(m.root_rows, 1);
        assert_eq!(m.entries.len(), 5);
        let doc = m.entry("document.svg").unwrap();
        assert_eq!(doc.role, Some(Role::Document));
        assert_eq!(doc.size, Some(3375));
        assert_eq!(doc.method, Some(Method::Deflate));
        assert_eq!(
            doc.blake3().unwrap().to_hex(),
            "e07d64cd960dc94e50d9030c9d1c3333d8b91c877bc113e535ade8d3ba5adc47"
        );
        assert_eq!(m.entry("META-INF/manifest.xml").unwrap().digest, None);
    }

    #[test]
    fn write_parse_is_a_fixed_point() {
        let m = parse(SPEC_EXAMPLE).unwrap();
        let a = m.to_xml().unwrap();
        let m2 = parse(&a).unwrap();
        assert_eq!(m, m2);
        assert_eq!(a, m2.to_xml().unwrap());
    }

    #[test]
    fn unknown_roles_attributes_and_children_are_preserved() {
        let src = r#"<?xml version="1.0"?>
<m:manifest xmlns:m="https://xarast.org/ns/manifest/1.0" xmlns:acme="urn:acme"
    xmlns="urn:default-ns"
    m:version="1.3" m:min-reader="1.0" m:profile="portable" acme:build="42" m:future="x">
  <m:requires><m:capability m:name="mesh-fill-v2" m:optional="true" acme:why="a&amp;b"/><acme:note>hi</acme:note></m:requires>
  <m:file-entry m:full-path="/" m:media-type="application/vnd.xarast+zip"/>
  <m:file-entry m:full-path="extensions/acme/data.bin" m:media-type="application/octet-stream"
      m:role="acme-data" acme:state="approved" plain="kept">
    <acme:signature alg="x"><inner xmlns="urn:inner">t&lt;</inner></acme:signature>
  </m:file-entry>
  <acme:root-note/>
  <!-- a comment -->
</m:manifest>"#;
        let m = parse(src).unwrap();
        assert_eq!(m.version.to_string(), "1.3");
        assert_eq!(m.requires.len(), 1);
        assert!(m.requires[0].optional);
        assert_eq!(m.requires[0].foreign_attrs[0].value, "a&b");
        assert_eq!(m.requires_foreign.len(), 1);
        assert!(
            m.foreign_attrs
                .iter()
                .any(|a| a.local == "build" && a.namespace == "urn:acme")
        );
        assert!(
            m.foreign_attrs
                .iter()
                .any(|a| a.local == "future" && a.namespace == MANIFEST_NS)
        );
        let e = m.entry("extensions/acme/data.bin").unwrap();
        assert_eq!(e.role, Some(Role::Other("acme-data".into())));
        assert_eq!(e.effective_role(), Role::Unknown);
        assert_eq!(e.foreign_attrs.len(), 2);
        assert_eq!(e.foreign_children.len(), 1);
        let frag = &e.foreign_children[0].xml;
        // Verbatim, plus the inherited declarations it needs.
        // Verbatim, plus the inherited declarations it uses. `inner` is
        // unprefixed, so the default namespace counts as used even though
        // `inner` redeclares it: over-inclusive, harmless, and stable.
        assert!(
            frag.starts_with(
                "<acme:signature xmlns=\"urn:default-ns\" xmlns:acme=\"urn:acme\" alg"
            ),
            "{frag}"
        );
        assert!(
            frag.ends_with(r#"alg="x"><inner xmlns="urn:inner">t&lt;</inner></acme:signature>"#),
            "{frag}"
        );
        assert_eq!(m.foreign_children.len(), 1);

        // Round trip: everything survives, and the second write is a fixed point.
        let out = m.to_xml().unwrap();
        let m2 = parse(&out).unwrap();
        assert_eq!(m2.requires, m.requires);
        assert_eq!(m2.entries[0].role, m.entries[0].role);
        // The prefix hints survive too, because they were free. The writer
        // emits foreign attributes sorted, so compare as sets.
        let sorted = |v: &[ForeignAttr]| {
            let mut v = v.to_vec();
            v.sort();
            v
        };
        assert_eq!(
            sorted(&m2.entries[0].foreign_attrs),
            sorted(&m.entries[0].foreign_attrs)
        );
        assert_eq!(
            m2.entries[0].foreign_children,
            m.entries[0].foreign_children
        );
        assert_eq!(m2.to_xml().unwrap(), out);
    }

    #[test]
    fn rejects_hostile_input() {
        let dtd = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa">]><mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"/>"#;
        assert_eq!(parse(dtd), Err(ManifestError::Dtd));

        let ent = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable" mf:generator="&lol;"/>"#;
        assert_eq!(
            parse(ent),
            Err(ManifestError::UndefinedEntity("lol".into()))
        );

        let ent_text = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable">&lol;</mf:manifest>"#;
        assert_eq!(
            parse(ent_text),
            Err(ManifestError::UndefinedEntity("lol".into()))
        );

        let mut deep = String::from(
            r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable">"#,
        );
        deep.push_str(&"<x>".repeat(300));
        deep.push_str(&"</x>".repeat(300));
        deep.push_str("</mf:manifest>");
        assert_eq!(parse(&deep), Err(ManifestError::TooDeep));

        let unbound = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable" q:x="1"/>"#;
        assert_eq!(
            parse(unbound),
            Err(ManifestError::UnboundPrefix("q".into()))
        );

        let slip = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"><mf:file-entry mf:full-path="../x" mf:media-type="a/b"/></mf:manifest>"#;
        assert!(matches!(
            parse(slip),
            Err(ManifestError::BadValue {
                attribute: "full-path",
                ..
            })
        ));

        let wrong_ns = r#"<mf:manifest xmlns:mf="urn:other" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"/>"#;
        assert_eq!(parse(wrong_ns), Err(ManifestError::NotAManifest));

        let two_roots = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"/><x/>"#;
        assert_eq!(parse(two_roots), Err(ManifestError::NotAManifest));

        let ctrl = "<mf:manifest xmlns:mf=\"https://xarast.org/ns/manifest/1.0\" mf:version=\"1.0\" mf:min-reader=\"1.0\" mf:profile=\"portable\" mf:generator=\"&#1;\"/>";
        assert!(parse(ctrl).is_err());

        assert!(parse("").is_err());
        assert!(parse("<unclosed").is_err());
        assert!(matches!(
            parse("\u{0}"),
            Err(ManifestError::InvalidCharacter)
        ));
    }

    #[test]
    fn names_are_validated() {
        // Found by fuzz_xarast_manifest: with no space before the first
        // attribute, quick-xml reads `file-entry0mf:full-path="…"` as the
        // element name.
        let glued = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"><mf:file-entry0mf:full-path="resources/x.png" mf:media-type="a/b"/></mf:manifest>"#;
        assert!(matches!(parse(glued), Err(ManifestError::Xml(_))));
        let empty_prefix = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable" :x="1"/>"#;
        assert!(matches!(parse(empty_prefix), Err(ManifestError::Xml(_))));
        let bad_attr = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable" a:b:c="1"/>"#;
        assert!(parse(bad_attr).is_err());
    }

    #[test]
    fn nested_rows_stay_nested() {
        // Found by fuzz_xarast_manifest: a `mf:file-entry` nested in the `/`
        // row was carried to the manifest level and became a real row.
        let src = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"><mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip" q="1"><mf:file-entry mf:full-path="/" mf:media-type="x/y"/></mf:file-entry><mf:requires><mf:capability mf:name="a"><mf:capability mf:name="b"/></mf:capability></mf:requires></mf:manifest>"#;
        let m = parse(src).unwrap();
        assert_eq!(m.root_rows, 1);
        assert_eq!(m.root_foreign_children.len(), 1);
        assert_eq!(m.root_foreign_attrs.len(), 1);
        assert_eq!(m.requires.len(), 1);
        assert_eq!(m.requires[0].foreign_children.len(), 1);
        let m1 = parse(&m.to_xml().unwrap()).unwrap();
        assert_eq!(m1, m);
    }

    #[test]
    fn a_bom_does_not_shift_fragments() {
        // Found by fuzz_xarast_manifest: with a BOM, every captured fragment
        // was cut three bytes early.
        let src = "\u{feff}<mf:manifest xmlns:mf=\"https://xarast.org/ns/manifest/1.0\" mf:version=\"1.0\" mf:min-reader=\"1.0\" mf:profile=\"portable\">\n  <mf:unknown a=\"1\"/>\n</mf:manifest>";
        let m = parse(src).unwrap();
        assert_eq!(
            m.foreign_children[0].xml,
            "<mf:unknown xmlns:mf=\"https://xarast.org/ns/manifest/1.0\" a=\"1\"/>"
        );
        assert_eq!(
            parse(&m.to_xml().unwrap()).unwrap().foreign_children,
            m.foreign_children
        );
        // Found by the same target: two BOMs (quick-xml skips both).
        assert!(parse(&format!("\u{feff}{src}")).is_err());
    }

    #[test]
    fn mandatory_attributes() {
        let no_profile = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0"/>"#;
        assert_eq!(
            parse(no_profile),
            Err(ManifestError::MissingAttribute {
                element: "manifest",
                attribute: "profile"
            })
        );
        // Unprefixed attributes are not ours: `version` here is foreign.
        let unprefixed = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" version="1.0" mf:min-reader="1.0" mf:profile="portable"/>"#;
        assert!(matches!(
            parse(unprefixed),
            Err(ManifestError::MissingAttribute {
                attribute: "version",
                ..
            })
        ));
        let bad_digest = r#"<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0" mf:version="1.0" mf:min-reader="1.0" mf:profile="portable"><mf:file-entry mf:full-path="a" mf:media-type="a/b" mf:digest="blake3-256" mf:digest-value="ABC"/></mf:manifest>"#;
        assert!(matches!(
            parse(bad_digest),
            Err(ManifestError::BadValue {
                attribute: "digest-value",
                ..
            })
        ));
    }

    #[test]
    fn attribute_escaping_round_trips() {
        let mut m = Manifest::new(Profile::Portable, Some("a\"b<c>&d\te\nf".into()));
        let mut e = FileEntry::new("extensions/x y/é.bin", "application/octet-stream");
        e.role = Some(Role::Extension);
        e.foreign_attrs.push(ForeignAttr {
            namespace: "urn:q".into(),
            local: "k".into(),
            prefix: Some("mf".into()),
            value: "v".into(),
        });
        m.entries.push(e);
        let xml = m.to_xml().unwrap();
        let back = parse(&xml).unwrap();
        assert_eq!(back.generator, m.generator);
        // The hint `mf` was taken, so the namespace got a generated prefix.
        assert_eq!(
            back.entries[0].foreign_attrs[0].prefix.as_deref(),
            Some("ns1")
        );
        assert_eq!(back.entries[0].foreign_attrs[0].namespace, "urn:q");
        assert_eq!(back.entries[0].full_path, "extensions/x y/é.bin");
    }
}
