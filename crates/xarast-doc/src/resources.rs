//! The document's resource tables.
//!
//! Everything the original kept as a `DocComponent` leaves the tree and lives
//! here, behind an [`Arc`] so that cloning a node — for undo, for the
//! clipboard, for a blend step — copies a handful of pointers.
//!
//! Two rules that are easy to get wrong:
//!
//! - **Deduplication is by SHA-256 of the decoded payload**, so importing the
//!   same bitmap twice yields one resource and one allocation.
//! - **Liveness is a sweep, not a reference count.** `Arc` counts references;
//!   whether a resource is still *wanted* is [`collect_unused`], run on save
//!   and on history eviction and never per edit — because a node detached by
//!   undo still holds its resources and must keep them.

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use slotmap::SlotMap;

use crate::kind::{ArrowSpec, NodeKind};

slotmap::new_key_type! {
    /// A bitmap resource.
    pub struct BitmapId;
    /// A dash pattern resource.
    pub struct DashId;
    /// An arrowhead resource.
    pub struct ArrowId;
}

/// A reference from the tree into a resource table.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ResourceRef {
    /// A palette colour.
    Colour(xarast_color::ColourId),
    /// A bitmap.
    Bitmap(BitmapId),
    /// A dash pattern.
    Dash(DashId),
    /// An arrowhead.
    Arrow(ArrowId),
}

/// A font file a document carries for display only: one face of a
/// `.xarast` package's `resources/fonts/` (`research/06 §6.7` rule 2),
/// as its `@font-face` rule names it.
///
/// The model never draws with it or interprets it: the application lays
/// the document's text out with these faces only where the machine lacks
/// the face itself (`docs/memory/text.md`, "Embedded fonts on read").
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EmbeddedFont {
    /// The family the face was drawn as (its `@font-face` `font-family`).
    pub family: Arc<str>,
    /// CSS weight the rule declares.
    pub weight: u16,
    /// Whether the rule declares an italic (or oblique) style.
    pub italic: bool,
    /// The file as stored: WOFF2, or a plain OpenType file.
    pub data: Arc<[u8]>,
}

/// How a bitmap's pixels are laid out.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, Default)]
pub struct BitmapInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per pixel: 1, 2, 4, 8, 24 or 32.
    pub bpp: u8,
    /// Horizontal resolution in dots per inch.
    pub dpi_x: u32,
    /// Vertical resolution in dots per inch.
    pub dpi_y: u32,
}

/// A bitmap's decoded pixels, plus its palette when it has one.
///
/// Decoding is `xarast-image`'s job; this crate only carries the bytes.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BitmapData {
    /// The pixels, row-major, top row first.
    pub pixels: Arc<[u8]>,
    /// The palette, for indexed bitmaps.
    pub palette: Arc<[xarast_color::Rgba8]>,
}

/// The container format encoded bytes arrived in.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum ImageFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// Windows BMP or DIB.
    Bmp,
    /// GIF.
    Gif,
    /// Something we did not recognise; the bytes are kept regardless.
    Unknown,
}

/// The encoded source bytes of a bitmap.
///
/// Keeping them is a fidelity requirement, not an optimisation: re-encoding an
/// embedded JPEG on save loses quality on every round trip. If the bytes are
/// here, the writer emits them verbatim.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OriginalEncoded {
    /// The container format.
    pub format: ImageFormat,
    /// The bytes, exactly as they arrived.
    pub bytes: Arc<[u8]>,
}

/// A bitmap that is generated rather than stored.
///
/// Not serialised: regenerated instead. Its cache key is the parameters
/// themselves.
#[derive(Clone, PartialEq, Debug)]
pub struct ProceduralSource {
    /// The generator's parameters.
    pub params: crate::fill::ProceduralParams,
    /// Whether it is a fractal rather than plain noise.
    pub fractal: bool,
}

/// One bitmap in the document.
#[derive(Clone, PartialEq, Debug)]
pub struct BitmapResource {
    /// The name shown in the bitmap gallery.
    pub name: Arc<str>,
    /// The pixel layout.
    pub info: BitmapInfo,
    /// The decoded pixels, copy-on-write.
    pub pixels: Arc<BitmapData>,
    /// The encoded source bytes, when we have them.
    pub original: Option<Arc<OriginalEncoded>>,
    /// The generator, when the bitmap is procedural.
    pub procedural: Option<ProceduralSource>,
    /// The palette index that is transparent, GIF style.
    pub transparent_index: Option<u8>,
}

impl BitmapResource {
    /// The SHA-256 of the payload: the deduplication key.
    ///
    /// It covers the decoded pixels **and the encoded original**. Two
    /// bitmaps that decode to the same pixels but arrived as different bytes
    /// are still two resources, because the writer emits `original`
    /// verbatim and collapsing them would silently discard one of the two
    /// encodings. It also matters while a decoder does not exist yet: the
    /// `.xar` importer stores the encoded bytes and leaves `pixels` empty
    /// until Phase 10, and without the original in the hash every bitmap in
    /// a file would deduplicate onto the first.
    #[must_use]
    pub fn content_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.info.width.to_le_bytes());
        h.update(self.info.height.to_le_bytes());
        h.update([self.info.bpp]);
        h.update(&*self.pixels.pixels);
        for c in self.pixels.palette.iter() {
            h.update([c.r, c.g, c.b, c.a]);
        }
        match &self.original {
            Some(o) => {
                h.update([1u8, o.format as u8]);
                h.update(&*o.bytes);
            }
            None => h.update([0u8]),
        }
        h.finalize().into()
    }
}

/// Every resource the document owns.
#[derive(Clone, Debug, Default)]
pub struct DocumentResources {
    /// Named and indexed colours.
    pub colours: xarast_color::ColourTable,
    bitmaps: SlotMap<BitmapId, BitmapResource>,
    dashes: SlotMap<DashId, xarast_geom::DashPattern>,
    arrows: SlotMap<ArrowId, ArrowSpec>,
    bitmap_by_hash: HashMap<[u8; 32], BitmapId>,
    /// Faces carried for display, in the order first seen.
    fonts: Vec<EmbeddedFont>,
}

impl DocumentResources {
    /// An empty set of tables.
    #[must_use]
    pub fn new() -> DocumentResources {
        DocumentResources::default()
    }

    /// Inserts a bitmap, deduplicating by the SHA-256 of its decoded payload.
    ///
    /// Inserting the same image twice returns the same [`BitmapId`] and keeps
    /// one allocation.
    pub fn insert_bitmap(&mut self, res: BitmapResource) -> BitmapId {
        let hash = res.content_hash();
        if let Some(id) = self.bitmap_by_hash.get(&hash) {
            return *id;
        }
        let id = self.bitmaps.insert(res);
        self.bitmap_by_hash.insert(hash, id);
        id
    }

    /// Looks a bitmap up.
    #[inline]
    #[must_use]
    pub fn bitmap(&self, id: BitmapId) -> Option<&BitmapResource> {
        self.bitmaps.get(id)
    }

    /// Replaces a bitmap's pixels, returning the previous ones.
    pub(crate) fn replace_bitmap_pixels(
        &mut self,
        id: BitmapId,
        pixels: Option<Arc<BitmapData>>,
    ) -> Option<Arc<BitmapData>> {
        let res = self.bitmaps.get_mut(id)?;
        let old = res.pixels.clone();
        if let Some(p) = pixels {
            let old_hash = res.content_hash();
            res.pixels = p;
            let new_hash = res.content_hash();
            self.bitmap_by_hash.remove(&old_hash);
            self.bitmap_by_hash.insert(new_hash, id);
        }
        Some(old)
    }

    /// Every bitmap, in slot order.
    pub fn bitmaps(&self) -> impl Iterator<Item = (BitmapId, &BitmapResource)> + '_ {
        self.bitmaps.iter()
    }

    /// Inserts a dash pattern.
    pub fn insert_dash(&mut self, d: xarast_geom::DashPattern) -> DashId {
        self.dashes.insert(d)
    }

    /// Looks a dash pattern up.
    #[inline]
    #[must_use]
    pub fn dash(&self, id: DashId) -> Option<&xarast_geom::DashPattern> {
        self.dashes.get(id)
    }

    /// Every dash pattern, in slot order.
    pub fn dashes(&self) -> impl Iterator<Item = (DashId, &xarast_geom::DashPattern)> + '_ {
        self.dashes.iter()
    }

    /// Inserts an arrowhead.
    pub fn insert_arrow(&mut self, a: ArrowSpec) -> ArrowId {
        self.arrows.insert(a)
    }

    /// Looks an arrowhead up.
    #[inline]
    #[must_use]
    pub fn arrow(&self, id: ArrowId) -> Option<&ArrowSpec> {
        self.arrows.get(id)
    }

    /// Every arrowhead, in slot order.
    pub fn arrows(&self) -> impl Iterator<Item = (ArrowId, &ArrowSpec)> + '_ {
        self.arrows.iter()
    }

    /// Adds a face carried for display. The same family with the same
    /// bytes is kept once; returns whether it was new.
    pub fn insert_font(&mut self, font: EmbeddedFont) -> bool {
        if self
            .fonts
            .iter()
            .any(|f| f.family == font.family && f.data == font.data)
        {
            return false;
        }
        self.fonts.push(font);
        true
    }

    /// The faces carried for display, in the order first seen.
    #[must_use]
    pub fn fonts(&self) -> &[EmbeddedFont] {
        &self.fonts
    }

    /// Whether a reference resolves.
    #[must_use]
    pub fn contains(&self, r: ResourceRef) -> bool {
        match r {
            ResourceRef::Colour(id) => self.colours.get(id).is_some(),
            ResourceRef::Bitmap(id) => self.bitmaps.contains_key(id),
            ResourceRef::Dash(id) => self.dashes.contains_key(id),
            ResourceRef::Arrow(id) => self.arrows.contains_key(id),
        }
    }

    /// How many resources of each kind there are: bitmaps, dashes, arrows.
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.bitmaps.len(), self.dashes.len(), self.arrows.len())
    }

    fn retain_bitmaps(&mut self, keep: &std::collections::HashSet<BitmapId>) -> usize {
        let before = self.bitmaps.len();
        self.bitmaps.retain(|id, _| keep.contains(&id));
        self.bitmap_by_hash
            .retain(|_, id| self.bitmaps.contains_key(*id));
        before - self.bitmaps.len()
    }
}

/// Collects every resource reference the node makes.
pub(crate) fn refs_of(kind: &NodeKind, out: &mut Vec<ResourceRef>) {
    match kind {
        NodeKind::Bitmap(b) => out.push(ResourceRef::Bitmap(b.image)),
        NodeKind::Guideline(g) => {
            if let Some(c) = g.colour {
                out.push(ResourceRef::Colour(c));
            }
        }
        NodeKind::Layer(l) => {
            if let Some(c) = l.guide_colour {
                out.push(ResourceRef::Colour(c));
            }
        }
        NodeKind::Attr(a) => a.value.resource_refs(out),
        _ => {}
    }
}

/// Sweeps resources that nothing alive in the arena references.
///
/// "Alive" includes nodes the history has detached but still retains, which is
/// exactly why this is a sweep and not a reference count: undoing a deletion
/// must find the resources still there.
///
/// Returns how many resources went.
pub fn collect_unused(doc: &mut crate::Document) -> usize {
    let mut keep_bitmaps: std::collections::HashSet<BitmapId> = std::collections::HashSet::new();
    let mut refs = Vec::new();
    for (_, data) in doc.tree.iter() {
        refs.clear();
        refs_of(&data.kind, &mut refs);
        for r in &refs {
            if let ResourceRef::Bitmap(b) = r {
                keep_bitmaps.insert(*b);
            }
        }
    }
    doc.resources.retain_bitmaps(&keep_bitmaps)
}
