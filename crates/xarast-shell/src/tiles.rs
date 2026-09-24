//! The canvas compositor: CPU-rasterised frames kept as tiles and moved by
//! the presenter at input time (XARA-T-0050, `render.md` "The GPU
//! decision").
//!
//! The render thread keeps rasterising whole frames (or the strips a pan
//! exposes). The shell cuts each frame into `256²` tiles of a **level**
//! (one zoom, one pixel grid) and keeps them. Every present composites the
//! tiles for the *current* view, so a pan or a `Draft` zoom is on screen
//! as soon as the input arrives, before the render thread has answered,
//! and the strips it rasterises afterwards fill the exposed areas.
//!
//! ```text
//!  RenderedFrame ─► TiledFrame ─► TilePlanner::accept ─► TileStore::upload (only what is new)
//!  Session view  ─► CanvasView ─► TilePlanner::placements ─► GPU encode │ compose_cpu + upload
//! ```
//!
//! Two stores implement the same operation, which is the capability
//! ladder's canvas half:
//!
//! * [`CanvasTier::GpuTiles`]: [`GpuTileCache`] on the shell's device;
//!   the composite is one instanced draw into the canvas texture, in the
//!   frame's encoder.
//! * [`CanvasTier::Cpu`]: the same tiles in memory, composited by
//!   [`compose_cpu`] and uploaded as one texture. Byte-identical to the
//!   GPU tier (`parity_tiles`), slower, and dependent on nothing but a
//!   device that can show a texture. Chosen by `XARAST_RENDERER=cpu`, when
//!   the tile cache cannot be created, or after repeated GPU errors.

use std::collections::HashMap;
use std::sync::Arc;

use xarast_render::{
    DeviceRect, GpuTileCache, GpuTileCacheConfig, Point64, Surface, TexelRect, TileGrid, TileKey,
    TilePlacement, Transform2D, compose_cpu,
};

use crate::paint::Painter;

/// A finished canvas frame, handed to the shell to be kept as tiles.
#[derive(Debug, Clone)]
pub struct TiledFrame {
    /// The pixels: premultiplied RGBA8, non-linear sRGB.
    pub surface: Surface,
    /// Document to frame pixels: the view the pixels show.
    pub transform: Transform2D,
    /// What the pixels are a picture of. Frames with equal keys show the
    /// same picture wherever they overlap (same document, scene epoch and
    /// backdrop). A new key forgets every tile, unless the frame repainted
    /// the last frame uploaded (`base`, same document): then the tiles
    /// under it are kept and only its `fresh` rectangles go up.
    pub content: (u64, u64),
    /// The frame's generation.
    pub generation: u64,
    /// The generation whose pixels this frame moved (a scroll) or kept (a
    /// repaint of an edit's damage).
    pub base: Option<u64>,
    /// The part of the surface that holds the picture.
    pub covered: DeviceRect,
    /// The parts rasterised for this frame; the rest of `covered` was
    /// moved from `base`.
    pub fresh: Vec<DeviceRect>,
}

/// The view the canvas shows now, which may be ahead of the last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasView {
    /// Where the canvas's top-left corner goes, in window device pixels.
    pub origin: (i32, i32),
    /// Canvas width in device pixels.
    pub width: u32,
    /// Canvas height in device pixels.
    pub height: u32,
    /// Document to canvas pixels.
    pub transform: Transform2D,
    /// Premultiplied colour where no tile lands: the pasteboard.
    pub backdrop: [u8; 4],
}

/// Which implementation presents the canvas. Shown in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasTier {
    /// Tiles resident on the GPU, composited by the GPU.
    GpuTiles,
    /// Tiles in memory, composited by the CPU and uploaded whole.
    Cpu,
}

impl CanvasTier {
    /// The status bar's name for it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            CanvasTier::GpuTiles => "GPU tiles",
            CanvasTier::Cpu => "CPU",
        }
    }
}

/// Where tiles are kept. The GPU atlas or memory; the planner does not
/// care which.
pub trait TileStore {
    /// The valid texels of a resident tile.
    fn valid(&self, key: &TileKey) -> Option<TexelRect>;
    /// Uploads `rect` of `src` to texel `at` of tile `key`; the valid area
    /// grows to the bounding box of the old one and the piece.
    fn upload(
        &mut self,
        key: TileKey,
        src: &Surface,
        rect: DeviceRect,
        at: [u32; 2],
    ) -> Result<(), String>;
    /// Forgets one tile.
    fn invalidate(&mut self, key: &TileKey);
    /// Forgets every tile `keep` refuses.
    fn retain(&mut self, keep: &mut dyn FnMut(&TileKey) -> bool);
    /// Forgets every tile.
    fn clear(&mut self);
    /// Tiles held at most.
    fn capacity(&self) -> u32;
}

/// Tiles in memory: the CPU tier, and the reference for tests.
#[derive(Debug)]
pub struct CpuTileStore {
    tile_size: u32,
    capacity: u32,
    tiles: HashMap<TileKey, CpuTile>,
    clock: u64,
}

#[derive(Debug)]
struct CpuTile {
    surface: Surface,
    valid: TexelRect,
    used: u64,
}

impl CpuTileStore {
    /// An empty store of at most `capacity` tiles of `tile_size`².
    pub fn new(tile_size: u32, capacity: u32) -> CpuTileStore {
        CpuTileStore {
            tile_size,
            capacity: capacity.max(1),
            tiles: HashMap::new(),
            clock: 0,
        }
    }

    /// Composites `placements` into `target` over `backdrop`, exactly as
    /// the GPU tier does.
    pub fn compose(
        &mut self,
        target: &mut Surface,
        placements: &[TilePlacement],
        backdrop: [u8; 4],
    ) -> u32 {
        self.clock += 1;
        for p in placements {
            if let Some(t) = self.tiles.get_mut(&p.key) {
                t.used = self.clock;
            }
        }
        let tiles = &self.tiles;
        compose_cpu(target, backdrop, placements, |k| {
            tiles.get(k).map(|t| &t.surface)
        })
    }
}

impl TileStore for CpuTileStore {
    fn valid(&self, key: &TileKey) -> Option<TexelRect> {
        self.tiles.get(key).map(|t| t.valid)
    }

    fn upload(
        &mut self,
        key: TileKey,
        src: &Surface,
        rect: DeviceRect,
        at: [u32; 2],
    ) -> Result<(), String> {
        let ts = self.tile_size;
        if rect.is_empty()
            || rect.intersection(src.bounds()) != rect
            || u64::from(at[0]) + u64::from(rect.width()) > u64::from(ts)
            || u64::from(at[1]) + u64::from(rect.height()) > u64::from(ts)
        {
            return Err(format!("piece {rect:?} at {at:?} does not fit a tile"));
        }
        if !self.tiles.contains_key(&key)
            && self.tiles.len() >= self.capacity as usize
            && let Some(oldest) = self
                .tiles
                .iter()
                .min_by_key(|(k, t)| (t.used, **k))
                .map(|(k, _)| *k)
        {
            self.tiles.remove(&oldest);
        }
        self.clock += 1;
        let piece = [at[0], at[1], at[0] + rect.width(), at[1] + rect.height()];
        let tile = self.tiles.entry(key).or_insert_with(|| CpuTile {
            surface: Surface::new(ts, ts),
            valid: piece,
            used: 0,
        });
        tile.valid = union(tile.valid, piece);
        tile.used = self.clock;
        let (sw, tw) = (src.width() as usize * 4, ts as usize * 4);
        let row_bytes = rect.width() as usize * 4;
        let src_data = src.data();
        let dst = tile.surface.data_mut();
        for row in 0..rect.height() as usize {
            // `rect` is inside `src`, so its corner is non-negative.
            let s = (rect.y0 as usize + row) * sw + rect.x0 as usize * 4;
            let d = (at[1] as usize + row) * tw + at[0] as usize * 4;
            dst[d..d + row_bytes].copy_from_slice(&src_data[s..s + row_bytes]);
        }
        Ok(())
    }

    fn invalidate(&mut self, key: &TileKey) {
        self.tiles.remove(key);
    }

    fn retain(&mut self, keep: &mut dyn FnMut(&TileKey) -> bool) {
        self.tiles.retain(|k, _| keep(k));
    }

    fn clear(&mut self) {
        self.tiles.clear();
    }

    fn capacity(&self) -> u32 {
        self.capacity
    }
}

/// Tiles in the GPU atlas.
#[derive(Debug)]
pub struct GpuTileStore {
    /// The cache on the shell's device.
    pub cache: GpuTileCache,
}

impl TileStore for GpuTileStore {
    fn valid(&self, key: &TileKey) -> Option<TexelRect> {
        self.cache.valid(key)
    }

    fn upload(
        &mut self,
        key: TileKey,
        src: &Surface,
        rect: DeviceRect,
        at: [u32; 2],
    ) -> Result<(), String> {
        self.cache
            .upload(key, src, rect, at)
            .map_err(|e| e.to_string())
    }

    fn invalidate(&mut self, key: &TileKey) {
        self.cache.invalidate(key);
    }

    fn retain(&mut self, keep: &mut dyn FnMut(&TileKey) -> bool) {
        self.cache.retain(keep);
    }

    fn clear(&mut self) {
        self.cache.clear();
    }

    fn capacity(&self) -> u32 {
        self.cache.config().capacity
    }
}

fn union(a: TexelRect, b: TexelRect) -> TexelRect {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn texel_area(r: TexelRect) -> u64 {
    u64::from(r[2].saturating_sub(r[0])) * u64::from(r[3].saturating_sub(r[1]))
}

/// The up to four rectangles of `all` outside `inner`: full-width bands
/// above and below, then the sides between them.
fn difference(all: DeviceRect, inner: DeviceRect) -> Vec<DeviceRect> {
    let inner = inner.intersection(all);
    if inner.is_empty() {
        return vec![all];
    }
    [
        DeviceRect::new(all.x0, all.y0, all.x1, inner.y0),
        DeviceRect::new(all.x0, inner.y1, all.x1, all.y1),
        DeviceRect::new(all.x0, inner.y0, inner.x0, inner.y1),
        DeviceRect::new(inner.x1, inner.y0, all.x1, inner.y1),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// Whether two rectangles together are exactly their bounding box, so
/// that growing a tile's valid area to it claims no texel neither holds.
fn joins_into_a_rectangle(a: TexelRect, b: TexelRect) -> bool {
    let i = xarast_render::texel_intersection(a, b);
    let overlap = if xarast_render::texel_rect_is_empty(i) {
        0
    } else {
        texel_area(i)
    };
    texel_area(union(a, b)) == texel_area(a) + texel_area(b) - overlap
}

/// Levels kept at once: the current zoom and the ones before it, drawn
/// under it so that a zoom-out shows what is still resident.
const MAX_LEVELS: usize = 3;

/// A translation this close to a whole pixel counts as whole (as in the
/// render thread's reuse, `reuse.rs`).
const WHOLE_PIXEL_EPS: f64 = 1e-3;

#[derive(Debug, Clone, Copy)]
struct Level {
    id: i32,
    /// Document to level space: the first frame's transform at this zoom
    /// and pixel grid.
    transform: Transform2D,
}

/// What one [`TilePlanner::accept`] uploaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UploadStats {
    /// Pieces uploaded.
    pub pieces: u32,
    /// Pixels uploaded.
    pub pixels: u64,
    /// Whether only what was new since the frame's base went up.
    pub incremental: bool,
}

/// Which tiles a frame fills and which tiles a view shows.
#[derive(Debug)]
pub struct TilePlanner {
    grid: TileGrid,
    levels: Vec<Level>,
    next_level: i32,
    content: Option<(u64, u64)>,
    /// The level and generation of the last frame uploaded.
    last: Option<(i32, u64)>,
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

impl TilePlanner {
    /// A planner for tiles of `tile_size`².
    pub fn new(tile_size: u32) -> TilePlanner {
        TilePlanner {
            grid: TileGrid { tile_size },
            levels: Vec::new(),
            next_level: 0,
            content: None,
            last: None,
        }
    }

    /// Forgets every level: the store was cleared or replaced.
    pub fn reset(&mut self) {
        self.levels.clear();
        self.content = None;
        self.last = None;
    }

    /// The level a frame drawn at `t` belongs to, and the whole-pixel
    /// offset of its pixels from level space. A frame at a new zoom, or at
    /// a fractional offset from every level of its zoom, starts a level.
    fn level_for(&mut self, t: Transform2D) -> (i32, [i32; 2]) {
        let b = t.to_affine().as_coeffs();
        let found = self.levels.iter().rposition(|l| {
            let a = l.transform.to_affine().as_coeffs();
            if !(0..4).all(|i| same(a[i], b[i])) {
                return false;
            }
            let (dx, dy) = (b[4] - a[4], b[5] - a[5]);
            (dx - dx.round()).abs() < WHOLE_PIXEL_EPS
                && (dy - dy.round()).abs() < WHOLE_PIXEL_EPS
                && dx.abs() < 1e9
                && dy.abs() < 1e9
        });
        if let Some(i) = found {
            let level = self.levels.remove(i);
            self.levels.push(level);
            let a = level.transform.to_affine().as_coeffs();
            // Whole and bounded by the test above.
            #[allow(clippy::cast_possible_truncation, reason = "|d| < 1e9, integral")]
            let d = [(b[4] - a[4]).round() as i32, (b[5] - a[5]).round() as i32];
            return (level.id, d);
        }
        let id = self.next_level;
        self.next_level = self.next_level.wrapping_add(1);
        self.levels.push(Level { id, transform: t });
        if self.levels.len() > MAX_LEVELS {
            self.levels.remove(0);
        }
        (id, [0, 0])
    }

    /// Keeps a frame's pixels as tiles, uploading only what the store does
    /// not already hold for this picture.
    pub fn accept(&mut self, f: &TiledFrame, store: &mut dyn TileStore) -> UploadStats {
        // A new picture drawn over the last frame uploaded (an edit's
        // repaint, XARA-T-0221): what the tiles hold under the frame is
        // that frame, so only the fresh pixels change.
        let edit = self
            .content
            .is_some_and(|c| c != f.content && c.0 == f.content.0)
            && f.base.is_some()
            && self.last.map(|(_, g)| g) == f.base;
        if self.content != Some(f.content) && !edit {
            self.forget(store, f.content);
        }
        let (mut level, mut d) = self.level_for(f.transform);
        let incremental = f.base.is_some_and(|b| self.last == Some((level, b)));
        let covered = f.covered.intersection(f.surface.bounds());
        if edit && !incremental {
            self.forget(store, f.content);
            (level, d) = self.level_for(f.transform);
        }
        let to_level = |r: DeviceRect| r.translated(-d[0], -d[1]);
        let covered_l = to_level(covered);
        // An edit that is incremental keeps only this level's tiles under
        // the frame: every other resident tile shows the old picture.
        let edit = edit && incremental;
        if edit {
            let under: std::collections::HashSet<TileKey> =
                self.grid.covering(level, covered_l).into_iter().collect();
            store.retain(&mut |k| under.contains(k));
            self.levels.retain(|l| l.id == level);
            self.content = Some(f.content);
        }
        let fresh_l: Vec<DeviceRect> = f
            .fresh
            .iter()
            .map(|r| to_level(r.intersection(covered)))
            .filter(|r| !r.is_empty())
            .collect();
        let mut stats = UploadStats {
            incremental,
            ..UploadStats::default()
        };
        let keys = self.grid.covering(level, covered_l);
        if keys.len() > store.capacity() as usize {
            // The store is sized for the view (`capacity_for`); a frame it
            // cannot hold is kept as far as it goes, and the LRU keeps the
            // last uploaded.
            tracing::warn!(
                tiles = keys.len(),
                capacity = store.capacity(),
                "canvas frame larger than the tile store"
            );
        }
        let upload = |store: &mut dyn TileStore,
                      stats: &mut UploadStats,
                      key: TileKey,
                      piece: DeviceRect,
                      tile: DeviceRect| {
            let at = [
                u32::try_from(piece.x0 - tile.x0).unwrap_or(0),
                u32::try_from(piece.y0 - tile.y0).unwrap_or(0),
            ];
            match store.upload(key, &f.surface, piece.translated(d[0], d[1]), at) {
                Ok(()) => {
                    stats.pieces += 1;
                    stats.pixels += piece.area();
                }
                Err(e) => tracing::warn!(error = %e, ?key, "tile upload refused"),
            }
        };
        for key in keys {
            let tile = self.grid.rect(key);
            let piece = tile.intersection(covered_l);
            if piece.is_empty() {
                continue;
            }
            let texels = |r: DeviceRect| -> TexelRect {
                [
                    u32::try_from(r.x0 - tile.x0).unwrap_or(0),
                    u32::try_from(r.y0 - tile.y0).unwrap_or(0),
                    u32::try_from(r.x1 - tile.x0).unwrap_or(0),
                    u32::try_from(r.y1 - tile.y0).unwrap_or(0),
                ]
            };
            let piece_t = texels(piece);
            // After an edit, a tile holding texels outside the frame holds
            // the old picture there: start it afresh.
            let stale = |v: TexelRect| {
                edit && !(v[0] >= piece_t[0]
                    && v[1] >= piece_t[1]
                    && v[2] <= piece_t[2]
                    && v[3] <= piece_t[3])
            };
            match store.valid(&key) {
                Some(v) if stale(v) => {
                    store.invalidate(&key);
                    upload(store, &mut stats, key, piece, tile);
                }
                Some(v) if incremental && joins_into_a_rectangle(v, piece_t) => {
                    // What the tile holds is this picture already: send
                    // what it lacks, and the fresh pixels over what it has.
                    let held = DeviceRect::new(
                        tile.x0 + i32::try_from(v[0]).unwrap_or(0),
                        tile.y0 + i32::try_from(v[1]).unwrap_or(0),
                        tile.x0 + i32::try_from(v[2]).unwrap_or(0),
                        tile.y0 + i32::try_from(v[3]).unwrap_or(0),
                    )
                    .intersection(piece);
                    for r in difference(piece, held) {
                        upload(store, &mut stats, key, r, tile);
                    }
                    for r in &fresh_l {
                        let sub = r.intersection(held);
                        if !sub.is_empty() {
                            upload(store, &mut stats, key, sub, tile);
                        }
                    }
                }
                Some(v) if joins_into_a_rectangle(v, piece_t) => {
                    upload(store, &mut stats, key, piece, tile)
                }
                Some(_) => {
                    store.invalidate(&key);
                    upload(store, &mut stats, key, piece, tile);
                }
                None => upload(store, &mut stats, key, piece, tile),
            }
        }
        self.last = Some((level, f.generation));
        stats
    }

    /// Forgets every tile and level: the picture is now `content`.
    fn forget(&mut self, store: &mut dyn TileStore, content: (u64, u64)) {
        store.clear();
        self.levels.clear();
        self.last = None;
        self.content = Some(content);
    }

    /// The placements that show `view` from the resident tiles, oldest
    /// level first so that the newest is drawn on top.
    pub fn placements(&self, view: &CanvasView, store: &dyn TileStore) -> Vec<TilePlacement> {
        let mut out = Vec::new();
        let Some(to_doc) = view.transform.invert() else {
            return out;
        };
        let (w, h) = (f64::from(view.width), f64::from(view.height));
        for level in &self.levels {
            // Canvas pixels to level space, to find the tiles in view.
            let m = to_doc.then(level.transform);
            let corners =
                [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| m.apply(Point64::new(x, y)));
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in corners {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
                continue;
            }
            let clamp = |v: f64| v.clamp(-1e9, 1e9);
            // Clamped to ±1e9, well inside i32.
            #[allow(clippy::cast_possible_truncation, reason = "clamped")]
            let area = DeviceRect::new(
                clamp(x0.floor()) as i32,
                clamp(y0.floor()) as i32,
                clamp(x1.ceil()) as i32,
                clamp(y1.ceil()) as i32,
            );
            let ts = f64::from(self.grid.tile_size.max(1));
            let count =
                (f64::from(area.width()) / ts + 2.0) * (f64::from(area.height()) / ts + 2.0);
            // A level far finer than the view (a deep zoom-out) would be
            // thousands of tiles, most of them not resident.
            if count > f64::from(store.capacity()) * 4.0 {
                continue;
            }
            for key in self.grid.covering(level.id, area) {
                let Some(valid) = store.valid(&key) else {
                    continue;
                };
                if let Some(p) = self
                    .grid
                    .placement(key, level.transform, view.transform, valid)
                {
                    out.push(p);
                }
            }
        }
        out
    }
}

/// Tiles a store should hold for a canvas of `w × h`: the view with a
/// tile of margin on each axis, twice over (the current level and the one
/// before it), and never fewer than the render crate's default.
#[must_use]
pub fn capacity_for(w: u32, h: u32, tile_size: u32) -> u32 {
    let ts = tile_size.max(1);
    let view = (w.div_ceil(ts) + 1) * (h.div_ceil(ts) + 1);
    (2 * view).max(GpuTileCacheConfig::default().capacity)
}

enum Store {
    Gpu(GpuTileStore),
    Cpu(CpuTileStore),
}

impl Store {
    fn as_dyn(&mut self) -> &mut dyn TileStore {
        match self {
            Store::Gpu(s) => s,
            Store::Cpu(s) => s,
        }
    }

    fn as_ref_dyn(&self) -> &dyn TileStore {
        match self {
            Store::Gpu(s) => s,
            Store::Cpu(s) => s,
        }
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Store::Gpu(s) => write!(f, "Gpu({} tiles)", s.cache.len()),
            Store::Cpu(s) => write!(f, "Cpu({} tiles)", s.tiles.len()),
        }
    }
}

/// The shell's canvas compositor: a planner, a store, the last frame and
/// the view to show.
#[derive(Debug)]
pub(crate) struct Compositor {
    tile_size: u32,
    planner: TilePlanner,
    store: Store,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    /// The last frame accepted, kept so that a change of store (a
    /// demotion, a bigger window) can refill the new one at once.
    last: Option<TiledFrame>,
    view: Option<CanvasView>,
    /// The canvas texture no longer shows `view` from the store.
    dirty: bool,
    /// Why the GPU tier was not used, or was left.
    pub(crate) reason: Option<String>,
    /// The CPU tier's composite, reused while the size holds.
    scratch: Option<Surface>,
}

impl Compositor {
    /// A compositor at the best tier `prefer_gpu` allows on this device.
    pub(crate) fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        prefer_gpu: bool,
    ) -> Compositor {
        let tile_size = xarast_render::GPU_TILE_SIZE;
        let capacity = GpuTileCacheConfig::default().capacity;
        let mut c = Compositor {
            tile_size,
            planner: TilePlanner::new(tile_size),
            store: Store::Cpu(CpuTileStore::new(tile_size, capacity)),
            device,
            queue,
            last: None,
            view: None,
            dirty: false,
            reason: None,
            scratch: None,
        };
        if prefer_gpu {
            c.store = c.gpu_store(capacity);
        } else {
            c.reason = Some("XARAST_RENDERER=cpu".to_owned());
        }
        tracing::info!(tier = c.tier().label(), reason = ?c.reason, "canvas compositor");
        c
    }

    fn gpu_store(&mut self, capacity: u32) -> Store {
        // A failure here is a validation error on a device that cannot
        // hold the atlas; catch it rather than let it reach the sink.
        let invalid = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let made = GpuTileCache::new(
            self.device.clone(),
            self.queue.clone(),
            GpuTileCacheConfig {
                tile_size: self.tile_size,
                capacity,
            },
        );
        let oom = pollster::block_on(oom.pop());
        let invalid = pollster::block_on(invalid.pop());
        match (made, oom.or(invalid)) {
            (Ok(cache), None) => Store::Gpu(GpuTileStore { cache }),
            (Err(e), _) => {
                self.reason = Some(format!("tile cache unavailable: {e}"));
                Store::Cpu(CpuTileStore::new(self.tile_size, capacity))
            }
            (Ok(_), Some(e)) => {
                self.reason = Some(format!("tile cache unavailable: {e}"));
                Store::Cpu(CpuTileStore::new(self.tile_size, capacity))
            }
        }
    }

    /// The tier in force.
    pub(crate) const fn tier(&self) -> CanvasTier {
        match self.store {
            Store::Gpu(_) => CanvasTier::GpuTiles,
            Store::Cpu(_) => CanvasTier::Cpu,
        }
    }

    /// Whether a view has been set, so the canvas pass is the
    /// compositor's.
    pub(crate) const fn is_active(&self) -> bool {
        self.view.is_some()
    }

    /// Falls back to the CPU tier for good, refilling it from the last
    /// frame. A no-op on the CPU tier.
    pub(crate) fn demote(&mut self, why: &str) {
        if self.tier() == CanvasTier::Cpu {
            return;
        }
        tracing::warn!(reason = why, "canvas falls back to CPU composition");
        self.reason = Some(why.to_owned());
        let capacity = self.store.as_ref_dyn().capacity();
        self.store = Store::Cpu(CpuTileStore::new(self.tile_size, capacity));
        self.refill();
    }

    fn refill(&mut self) {
        self.planner.reset();
        if let Some(f) = self.last.take() {
            self.planner.accept(&f, self.store.as_dyn());
            self.last = Some(f);
        }
        self.dirty = true;
    }

    /// Keeps a frame's pixels.
    pub(crate) fn accept(&mut self, frame: TiledFrame) -> UploadStats {
        let stats = self.planner.accept(&frame, self.store.as_dyn());
        self.last = Some(frame);
        self.dirty = true;
        stats
    }

    /// Sets the view to show; grows the store when the canvas outgrows it.
    pub(crate) fn set_view(&mut self, view: CanvasView) {
        if self.view == Some(view) {
            return;
        }
        let want = capacity_for(view.width, view.height, self.tile_size);
        if want > self.store.as_ref_dyn().capacity() {
            tracing::info!(capacity = want, "growing the tile store");
            self.store = match self.store {
                Store::Gpu(_) => self.gpu_store(want),
                Store::Cpu(_) => Store::Cpu(CpuTileStore::new(self.tile_size, want)),
            };
            self.refill();
        }
        self.view = Some(view);
        self.dirty = true;
    }

    /// Stops compositing: the canvas pass goes back to whatever the
    /// painter was given.
    pub(crate) fn deactivate(&mut self) {
        self.view = None;
        self.last = None;
        self.planner.reset();
        self.store.as_dyn().clear();
    }

    /// Readies the canvas texture before the frame's encoder exists: the
    /// CPU tier composites and uploads here. Returns whether the GPU tier
    /// has a composite to encode.
    pub(crate) fn prepare(&mut self, painter: &mut Painter) -> bool {
        let Some(view) = self.view else { return false };
        if !self.dirty {
            return false;
        }
        let size = [view.width.max(1), view.height.max(1)];
        let placements = self.planner.placements(&view, self.store.as_ref_dyn());
        match &mut self.store {
            Store::Gpu(_) => {
                painter.canvas_target(&self.device, size, view.origin);
                true
            }
            Store::Cpu(store) => {
                let mut target = match self.scratch.take() {
                    Some(s) if s.width() == size[0] && s.height() == size[1] => s,
                    _ => Surface::new(size[0], size[1]),
                };
                store.compose(&mut target, &placements, view.backdrop);
                painter.upload_canvas(&self.device, &self.queue, &target, view.origin);
                self.scratch = Some(target);
                self.dirty = false;
                false
            }
        }
    }

    /// Records the GPU tier's composite into the canvas texture, on the
    /// frame's encoder (before the canvas and interface pass).
    pub(crate) fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, painter: &Painter) {
        let Some(view) = self.view else { return };
        let placements = self.planner.placements(&view, self.store.as_ref_dyn());
        let (Store::Gpu(store), Some(target)) = (&mut self.store, painter.canvas_texture()) else {
            return;
        };
        match store
            .cache
            .encode(encoder, target, &placements, view.backdrop)
        {
            Ok(_) => self.dirty = false,
            Err(e) => tracing::warn!(error = %e, "tile composite refused"),
        }
    }

    /// The view shown, with its origin moved by the layout.
    pub(crate) fn move_to(&mut self, origin: (i32, i32)) {
        if let Some(v) = self.view.as_mut() {
            v.origin = origin;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: u32 = 16;

    /// A frame of a synthetic picture: every level-space pixel has a
    /// colour derived from its document position, so any two frames of the
    /// same view agree and a misplaced tile shows.
    fn picture(t: Transform2D, w: u32, h: u32) -> Surface {
        let inv = t.invert().unwrap();
        let mut s = Surface::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = inv.apply(Point64::new(f64::from(x) + 0.5, f64::from(y) + 0.5));
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (a, b) = (
                    (p.x.floor() as i64 & 0xff) as u8,
                    (p.y.floor() as i64 & 0xff) as u8,
                );
                s.set_pixel(x as i32, y as i32, [a, b, a ^ b, 255]);
            }
        }
        s
    }

    fn frame(
        t: Transform2D,
        w: u32,
        h: u32,
        generation: u64,
        base: Option<u64>,
        fresh: Vec<DeviceRect>,
    ) -> TiledFrame {
        TiledFrame {
            surface: picture(t, w, h),
            transform: t,
            content: (1, 1),
            generation,
            base,
            covered: DeviceRect::from_size(w, h),
            fresh,
        }
    }

    fn view(t: Transform2D, w: u32, h: u32) -> CanvasView {
        CanvasView {
            origin: (0, 0),
            width: w,
            height: h,
            transform: t,
            backdrop: [9, 9, 9, 255],
        }
    }

    fn composite(p: &TilePlanner, s: &mut CpuTileStore, v: &CanvasView) -> Surface {
        let mut out = Surface::new(v.width, v.height);
        let pl = p.placements(v, s);
        s.compose(&mut out, &pl, v.backdrop);
        out
    }

    #[test]
    fn a_frame_composited_at_its_own_view_is_itself() {
        let t = Transform2D::new([1.0, 0.0, 0.0, 1.0, 3.0, -5.0]);
        let f = frame(t, 50, 37, 1, None, vec![DeviceRect::from_size(50, 37)]);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 64);
        let st = p.accept(&f, &mut s);
        assert!(!st.incremental);
        assert_eq!(st.pixels, 50 * 37);
        assert_eq!(composite(&p, &mut s, &view(t, 50, 37)), f.surface);
    }

    #[test]
    fn a_pan_is_shown_before_the_new_frame_and_filled_by_its_strips() {
        let t = Transform2D::new([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let f = frame(t, 48, 40, 1, None, vec![DeviceRect::from_size(48, 40)]);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 64);
        p.accept(&f, &mut s);

        // The input: a pan by (7, -3). Composited at once, the moved
        // pixels are right and the exposed strips are backdrop.
        let panned = t.then(Transform2D::translate(7.0, -3.0));
        let early = composite(&p, &mut s, &view(panned, 48, 40));
        let want = picture(panned, 48, 40);
        assert_eq!(early.pixel(20, 20), want.pixel(20, 20));
        assert_eq!(early.pixel(2, 20), Some([9, 9, 9, 255]));

        // The render thread's answer: a scroll with two strips.
        let strips = vec![DeviceRect::new(0, 0, 7, 40), DeviceRect::new(0, 37, 48, 40)];
        let g = frame(panned, 48, 40, 2, Some(1), strips);
        let st = p.accept(&g, &mut s);
        assert!(st.incremental);
        // The strips (their shared corner twice), plus the partial tiles at
        // the trailing edge, which cannot grow their valid area into a
        // rectangle and start afresh: far from a whole frame.
        assert!(st.pixels >= 7 * 40 + 48 * 3, "{st:?}");
        assert!(st.pixels < 48 * 40 / 3, "{st:?}");
        assert_eq!(composite(&p, &mut s, &view(panned, 48, 40)), want);
    }

    #[test]
    fn a_frame_whose_base_was_never_seen_is_uploaded_whole() {
        let t = Transform2D::scale(1.0);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 64);
        p.accept(
            &frame(t, 40, 40, 1, None, vec![DeviceRect::from_size(40, 40)]),
            &mut s,
        );
        let panned = t.then(Transform2D::translate(-5.0, 0.0));
        let g = frame(
            panned,
            40,
            40,
            3,
            Some(2),
            vec![DeviceRect::new(35, 0, 40, 40)],
        );
        let st = p.accept(&g, &mut s);
        assert!(!st.incremental);
        assert_eq!(composite(&p, &mut s, &view(panned, 40, 40)), g.surface);
    }

    #[test]
    fn a_zoom_resamples_the_resident_level_and_a_new_level_goes_on_top() {
        let t = Transform2D::scale(1.0);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 256);
        p.accept(
            &frame(t, 64, 64, 1, None, vec![DeviceRect::from_size(64, 64)]),
            &mut s,
        );
        let zoomed = t.then(Transform2D::scale(2.0));
        let early = composite(&p, &mut s, &view(zoomed, 64, 64));
        // Nearest resampling of level 0: pixel (2x, 2y) shows (x, y).
        let f0 = picture(t, 64, 64);
        assert_eq!(early.pixel(21, 9), f0.pixel(10, 4));
        // The Final at the new zoom replaces it exactly.
        let g = frame(zoomed, 64, 64, 2, None, vec![DeviceRect::from_size(64, 64)]);
        p.accept(&g, &mut s);
        assert_eq!(composite(&p, &mut s, &view(zoomed, 64, 64)), g.surface);
        // Zooming back out shows the old level again, still resident.
        let back = composite(&p, &mut s, &view(t, 64, 64));
        assert_eq!(back, f0);
    }

    #[test]
    fn a_fractional_offset_starts_a_level_and_new_content_forgets_all() {
        let t = Transform2D::scale(1.0);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 256);
        p.accept(
            &frame(t, 32, 32, 1, None, vec![DeviceRect::from_size(32, 32)]),
            &mut s,
        );
        let half = t.then(Transform2D::translate(0.5, 0.0));
        let g = frame(half, 32, 32, 2, None, vec![DeviceRect::from_size(32, 32)]);
        p.accept(&g, &mut s);
        assert_eq!(p.levels.len(), 2);
        assert_eq!(composite(&p, &mut s, &view(half, 32, 32)), g.surface);
        let mut h = frame(t, 32, 32, 3, None, vec![DeviceRect::from_size(32, 32)]);
        h.content = (1, 2);
        p.accept(&h, &mut s);
        assert_eq!(p.levels.len(), 1);
        assert_eq!(s.tiles.len(), 4);
    }

    /// `f`'s picture with `r` painted a flat colour: the same frame after
    /// an edit that recoloured what is under `r`.
    fn edited(f: &TiledFrame, r: DeviceRect, generation: u64) -> TiledFrame {
        let mut g = f.clone();
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                g.surface.set_pixel(x, y, [200, 10, 10, 255]);
            }
        }
        g.content = (f.content.0, f.content.1 + 1);
        g.generation = generation;
        g.base = Some(f.generation);
        g.fresh = vec![r];
        g
    }

    #[test]
    fn an_edit_uploads_only_its_damage_and_forgets_what_is_out_of_view() {
        let t = Transform2D::scale(1.0);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 256);
        let f = frame(t, 48, 40, 1, None, vec![DeviceRect::from_size(48, 40)]);
        p.accept(&f, &mut s);
        // A pan leaves tiles resident beyond the view.
        let panned = t.then(Transform2D::translate(-16.0, 0.0));
        let strip = vec![DeviceRect::new(32, 0, 48, 40)];
        let g = frame(panned, 48, 40, 2, Some(1), strip);
        p.accept(&g, &mut s);
        let resident = s.tiles.len();

        let damage = DeviceRect::new(5, 6, 13, 20);
        let h = edited(&g, damage, 3);
        let st = p.accept(&h, &mut s);
        assert!(st.incremental, "{st:?}");
        // The damage (split across the tiles it crosses), and nothing more.
        assert_eq!(st.pixels, damage.area(), "{st:?}");
        assert_eq!(composite(&p, &mut s, &view(panned, 48, 40)), h.surface);
        // The column that scrolled out held the old picture: forgotten.
        assert!(s.tiles.len() < resident);
        let back = composite(&p, &mut s, &view(t, 48, 40));
        assert_eq!(back.pixel(3, 3), Some([9, 9, 9, 255]));
    }

    #[test]
    fn an_edit_over_a_frame_never_seen_is_uploaded_whole() {
        let t = Transform2D::scale(1.0);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 256);
        let f = frame(t, 40, 40, 1, None, vec![DeviceRect::from_size(40, 40)]);
        p.accept(&f, &mut s);
        // Its base is generation 2, which was dropped before collection.
        let mut g = f.clone();
        g.generation = 2;
        let h = edited(&g, DeviceRect::new(0, 0, 8, 8), 3);
        let st = p.accept(&h, &mut s);
        assert!(!st.incremental);
        assert_eq!(st.pixels, 40 * 40);
        assert_eq!(composite(&p, &mut s, &view(t, 40, 40)), h.surface);
    }

    #[test]
    fn an_edit_restarts_a_tile_that_holds_texels_outside_the_frame() {
        let t = Transform2D::scale(1.0);
        let mut p = TilePlanner::new(TS);
        let mut s = CpuTileStore::new(TS, 256);
        // A frame at a whole-pixel offset from the tile grid, then a pan by
        // 5: the edge tiles hold texels from both frames.
        let f = frame(t, 40, 40, 1, None, vec![DeviceRect::from_size(40, 40)]);
        p.accept(&f, &mut s);
        let panned = t.then(Transform2D::translate(-5.0, 0.0));
        let g = frame(
            panned,
            40,
            40,
            2,
            Some(1),
            vec![DeviceRect::new(35, 0, 40, 40)],
        );
        p.accept(&g, &mut s);
        let h = edited(&g, DeviceRect::new(20, 20, 24, 24), 3);
        let st = p.accept(&h, &mut s);
        assert!(st.incremental);
        // The left column of tiles restarts; the rest takes the damage only.
        assert!(st.pixels > 16, "{st:?}");
        assert!(st.pixels < 40 * 40 / 2, "{st:?}");
        assert_eq!(composite(&p, &mut s, &view(panned, 40, 40)), h.surface);
        let back = composite(&p, &mut s, &view(t, 40, 40));
        assert_eq!(
            back.pixel(1, 1),
            Some([9, 9, 9, 255]),
            "old texels forgotten"
        );
    }

    #[test]
    fn disjoint_pieces_of_one_tile_start_it_afresh() {
        let mut s = CpuTileStore::new(TS, 8);
        let key = TileKey {
            level: 0,
            tx: 0,
            ty: 0,
        };
        let src = Surface::filled(TS, TS, [1, 2, 3, 255]);
        s.upload(key, &src, DeviceRect::new(0, 0, 4, 16), [0, 0])
            .unwrap();
        assert!(!joins_into_a_rectangle(
            s.valid(&key).unwrap(),
            [10, 0, 16, 16]
        ));
        assert!(joins_into_a_rectangle(
            s.valid(&key).unwrap(),
            [4, 0, 16, 16]
        ));
        assert!(joins_into_a_rectangle([0, 0, 8, 8], [0, 0, 4, 4]));
    }

    /// A headless device on whatever adapter there is, or `None`.
    fn device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        // Real GPUs are opt-in: an unattended `cargo test` must never touch the
        // maintainer's display driver (concurrent runs once hung the desktop).
        if std::env::var("XARAST_GPU_TESTS").as_deref() != Ok("1") {
            eprintln!("skipping: set XARAST_GPU_TESTS=1 to run GPU tests");
            return None;
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all().with_env(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .ok()?;
        let (d, q) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some((Arc::new(d), Arc::new(q)))
    }

    #[test]
    fn the_gpu_tier_composites_byte_for_byte_as_the_cpu_tier() {
        let Some((device, queue)) = device() else {
            eprintln!("skipped: no GPU adapter");
            return;
        };
        let cache = GpuTileCache::new(
            device.clone(),
            queue.clone(),
            GpuTileCacheConfig {
                tile_size: TS,
                capacity: 256,
            },
        )
        .unwrap();
        let mut gpu = GpuTileStore { cache };
        let mut cpu = CpuTileStore::new(TS, 256);
        let (mut pg, mut pc) = (TilePlanner::new(TS), TilePlanner::new(TS));
        // No pixel centre lands on a whole document unit, so the test
        // picture does not depend on rounding and any frame of it agrees.
        let t = Transform2D::new([1.5, 0.0, 0.0, 1.5, 2.3, 1.3]);
        let (w, h) = (70, 45);
        let f = frame(t, w, h, 1, None, vec![DeviceRect::from_size(w, h)]);
        pg.accept(&f, &mut gpu);
        pc.accept(&f, &mut cpu);
        let panned = t.then(Transform2D::translate(9.0, -4.0));
        let g = frame(
            panned,
            w,
            h,
            2,
            Some(1),
            vec![DeviceRect::new(0, 0, 9, 45), DeviceRect::new(0, 41, 70, 45)],
        );
        pg.accept(&g, &mut gpu);
        pc.accept(&g, &mut cpu);
        // A pan ahead of the frames, a zoom in and a zoom out.
        for v in [
            panned.then(Transform2D::translate(-3.4, 5.6)),
            panned.then(Transform2D::scale(1.7)),
            panned.then(Transform2D::scale(0.6)),
            panned,
        ] {
            let view = view(v, w, h);
            let want = composite(&pc, &mut cpu, &view);
            let target = xarast_render::backend::gpu_tiles::create_target(&device, w, h);
            let placements = pg.placements(&view, &gpu);
            gpu.cache
                .compose(&target, &placements, view.backdrop)
                .unwrap();
            let got =
                xarast_render::backend::gpu_tiles::read_back(&device, &queue, &target).unwrap();
            assert_eq!(got, want, "the tiers differ at {v:?}");
        }
        let back = composite(&pc, &mut cpu, &view(panned, w, h));
        let diff: Vec<(i32, i32)> = (0..h as i32)
            .flat_map(|y| (0..w as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| back.pixel(x, y) != g.surface.pixel(x, y))
            .collect();
        assert!(
            diff.is_empty(),
            "{} pixels differ, first {:?}",
            diff.len(),
            &diff[..diff.len().min(8)]
        );
    }

    #[test]
    fn the_store_is_sized_for_the_view_twice() {
        assert_eq!(capacity_for(1920, 1080, 256), 128);
        assert_eq!(capacity_for(3840, 2160, 256), 2 * 16 * 10);
    }

    #[test]
    fn the_lru_keeps_the_newest_tiles() {
        let mut s = CpuTileStore::new(TS, 2);
        let src = Surface::filled(TS, TS, [1, 2, 3, 255]);
        for tx in 0..3 {
            s.upload(
                TileKey {
                    level: 0,
                    tx,
                    ty: 0,
                },
                &src,
                src.bounds(),
                [0, 0],
            )
            .unwrap();
        }
        assert!(
            s.valid(&TileKey {
                level: 0,
                tx: 0,
                ty: 0
            })
            .is_none()
        );
        assert!(
            s.valid(&TileKey {
                level: 0,
                tx: 2,
                ty: 0
            })
            .is_some()
        );
    }
}
