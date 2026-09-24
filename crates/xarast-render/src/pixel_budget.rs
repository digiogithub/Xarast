//! The pixel memory budget: how many bytes of decoded bitmap levels the
//! renderer keeps resident, and what it does when there are too many
//! (phase 10, W10.5).
//!
//! # What is counted
//!
//! Every [`ImageRef`](crate::ImageRef) owns a store of **levels**: the
//! full-resolution base (level 0) and its mip pyramid (`resample`). Each
//! level is either resident or not. A store splits its levels at its
//! **proxy** index `p`:
//!
//! - levels `p..` (the proxy and everything smaller) are **never evicted**
//!   once built. They are what a zoomed-out view samples, so a document
//!   with many large photographs can always draw them without touching
//!   disk or re-decoding.
//! - levels `..p` (the base and the reductions larger than the proxy) are
//!   **evictable**. Their resident bytes are what the budget limits.
//!
//! # The proxy rule
//!
//! A store starts with the largest pyramid level whose long edge is at
//! most [`BudgetConfig::proxy_default`] (256) as its proxy. Every time a
//! primitive samples levels `lo..=hi`, the proxy moves up to
//! `max(lo, cap)` if that is larger, where `cap` is the largest level
//! whose long edge is at most [`BudgetConfig::proxy_cap`] (2048): the
//! proxy follows the resolution the image is actually drawn at on canvas,
//! grows when it is scaled up, never shrinks, and never exceeds 2048 on
//! the long edge. An image no larger than the cap that has been drawn at
//! or above its own size therefore keeps its base resident for good.
//!
//! # Eviction
//!
//! After any store gains resident bytes, [`PixelBudget::enforce`] evicts
//! whole stores, least recently sampled first, until the evictable total is
//! at most the limit. Evicting a store drops its levels `..p`. When it drops
//! the base and no level `p..` is resident yet (an image nobody prepared),
//! it first reduces the base down to the proxy, so that something smaller
//! is always there to draw from without waiting. A sampler
//! holds its own `Arc`s to the levels it pinned, so eviction never pulls
//! pixels from under a primitive being drawn; the memory is freed when the
//! last pin goes.
//!
//! # Re-materialisation, byte for byte
//!
//! A level that is needed and not resident comes back from:
//!
//! 1. the next larger resident level, by the same 2 × 2 reduction that
//!    built it (so it is bit-identical), or, for the base,
//! 2. its **spill file**, when it has one, or
//! 3. its [`PixelSource`], which must reproduce the exact bytes first
//!    registered (the walker re-decodes the encoded original).
//!
//! A base with a source that says it is cheap ([`PixelSource::is_cheap`],
//! e.g. a copy of pixels the document holds anyway) is simply dropped. Any
//! other base is written to the spill directory the first time it is
//! evicted (`spill`), once; a base whose spill fails and that has no source
//! stays resident, so there is always a way back. Nothing here may change
//! a rendered pixel: the byte-for-byte tests in `tests/pixel_budget.rs`
//! and in `xarast-app` render under a tiny budget and compare.
//!
//! # Drawing without waiting ([`MissingLevels`])
//!
//! Bringing a base back costs a spill read or a decode. A render that must
//! not wait for that (the interactive render thread) asks for
//! [`MissingLevels::Substitute`]: a level that is not resident and cannot
//! be rebuilt from a larger resident one is then **not** re-materialised;
//! the sampler draws the best resident level instead (at worst the proxy,
//! which is always resident once the base has been evicted) and the image
//! is stamped with [`substitution_tick`]. The caller finds the stamped
//! images with [`ImageRef::substituted_since`](crate::ImageRef::substituted_since),
//! brings their bases back off its thread
//! ([`ImageRef::rematerialise`](crate::ImageRef::rematerialise)) and
//! redraws what they cover. Reductions from a resident larger level are
//! still done in place: they are arithmetic, not I/O, and exact. Export,
//! thumbnails and the tests use the default [`MissingLevels::Materialise`],
//! which is byte-identical to an unlimited budget.
//!
//! # Locks
//!
//! Each store has a mutex, the budget has one for its ledger. A store's
//! lock may be held while taking the ledger's; the ledger's is never held
//! while taking a store's. Stores are evicted with the ledger unlocked.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};

use crate::spill::{SpillDir, SpillFile};

/// The environment variable that sets the global budget, in MiB.
pub const ENV_BUDGET_MB: &str = "XARAST_PIXEL_BUDGET_MB";

/// The largest proxy, on the long edge, in texels (the phase's cap).
pub const PROXY_CAP: u32 = 2048;

/// The proxy of an image nobody has drawn yet, on the long edge.
pub const PROXY_DEFAULT: u32 = 256;

const MIB: u64 = 1024 * 1024;

/// What a sampler does about a level that is not resident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MissingLevels {
    /// Re-materialise it now, byte for byte, whatever it costs: the
    /// output is identical to an unlimited budget.
    #[default]
    Materialise,
    /// Rebuild it only when that is a reduction of a resident larger
    /// level; if it needs the base back (a spill read or a decode), draw
    /// the best resident level instead and stamp the image
    /// ([`substitution_tick`]). The pixels are then not the exact ones
    /// until the caller re-materialises and redraws.
    Substitute,
}

/// Every substitution takes a new value of this counter, so a caller that
/// read it before a render can tell which images were substituted during
/// that render.
static SUBSTITUTIONS: AtomicU64 = AtomicU64::new(1);

/// The substitution clock now: an image stamped after this value was
/// drawn from a substitute after the call ([`MissingLevels::Substitute`]).
#[must_use]
pub fn substitution_tick() -> u64 {
    SUBSTITUTIONS.load(Ordering::SeqCst)
}

/// Where re-produced pixels come from after an eviction.
///
/// `materialise` must return **exactly** the straight RGBA8 bytes the
/// image was created with, every time: a decode of the same encoded bytes
/// under the same limits is (every decoder here is deterministic).
pub trait PixelSource: Send + Sync {
    /// The base level again, `width · height · 4` bytes, or `None` when it
    /// can no longer be produced.
    fn materialise(&self) -> Option<Vec<u8>>;

    /// Whether producing it is cheap enough (a copy, not a decode) that
    /// spilling it to disk would cost more than it saves.
    fn is_cheap(&self) -> bool {
        false
    }
}

/// A [`PixelSource`] made from a closure.
pub struct FnSource<F> {
    f: F,
    cheap: bool,
}

impl<F: Fn() -> Option<Vec<u8>> + Send + Sync> FnSource<F> {
    /// A source that costs a decode: its base is spilled when evicted.
    pub const fn expensive(f: F) -> FnSource<F> {
        FnSource { f, cheap: false }
    }

    /// A source that costs a copy: its base is dropped when evicted.
    pub const fn cheap(f: F) -> FnSource<F> {
        FnSource { f, cheap: true }
    }
}

impl<F: Fn() -> Option<Vec<u8>> + Send + Sync> PixelSource for FnSource<F> {
    fn materialise(&self) -> Option<Vec<u8>> {
        (self.f)()
    }

    fn is_cheap(&self) -> bool {
        self.cheap
    }
}

impl<F> fmt::Debug for FnSource<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FnSource")
            .field("cheap", &self.cheap)
            .finish_non_exhaustive()
    }
}

/// How a [`PixelBudget`] behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetConfig {
    /// The most evictable bytes kept resident. `u64::MAX` never evicts.
    pub limit_bytes: u64,
    /// The largest proxy, long edge, texels ([`PROXY_CAP`]).
    pub proxy_cap: u32,
    /// The proxy of an image not yet drawn ([`PROXY_DEFAULT`]).
    pub proxy_default: u32,
    /// Where the spill directory goes; `None` is [`crate::spill::default_root`].
    pub spill_root: Option<PathBuf>,
    /// Whether evicted bases without a cheap source may be spilled.
    pub spill: bool,
}

impl BudgetConfig {
    /// The product default: [`default_limit`] (or `XARAST_PIXEL_BUDGET_MB`),
    /// the phase's proxy sizes, spilling on.
    #[must_use]
    pub fn from_env() -> BudgetConfig {
        let limit_bytes = std::env::var(ENV_BUDGET_MB)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map_or_else(default_limit, |mb| mb.saturating_mul(MIB));
        BudgetConfig {
            limit_bytes,
            ..BudgetConfig::unlimited()
        }
    }

    /// Never evicts.
    #[must_use]
    pub fn unlimited() -> BudgetConfig {
        BudgetConfig {
            limit_bytes: u64::MAX,
            proxy_cap: PROXY_CAP,
            proxy_default: PROXY_DEFAULT,
            spill_root: None,
            spill: true,
        }
    }
}

/// The default limit: a quarter of physical memory, clamped to
/// 512 MiB – 4 GiB; 1 GiB where physical memory cannot be read.
#[must_use]
pub fn default_limit() -> u64 {
    physical_memory().map_or(1024 * MIB, |m| (m / 4).clamp(512 * MIB, 4096 * MIB))
}

fn physical_memory() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// What the budget has done so far. Counters only grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetStats {
    /// The limit on evictable bytes.
    pub limit_bytes: u64,
    /// Evictable bytes resident now.
    pub evictable_bytes: u64,
    /// Proxy bytes resident now (never evicted).
    pub proxy_bytes: u64,
    /// The most evictable + proxy bytes ever resident at once.
    pub peak_bytes: u64,
    /// Live images registered.
    pub images: u64,
    /// Stores evicted.
    pub evictions: u64,
    /// Bytes those evictions freed.
    pub evicted_bytes: u64,
    /// Bases re-produced by their source.
    pub from_source: u64,
    /// Bases read back from a spill file.
    pub from_spill: u64,
    /// Levels rebuilt by reduction from a larger level.
    pub levels_rebuilt: u64,
    /// Spill files written.
    pub spill_writes: u64,
    /// Bytes written to spill files.
    pub spill_bytes: u64,
    /// Spill writes or reads that failed.
    pub spill_failures: u64,
    /// Bases that could not be re-produced at all (drawn transparent).
    pub lost: u64,
    /// Samplers that drew a smaller resident level instead of waiting for
    /// a base ([`MissingLevels::Substitute`]).
    pub substituted: u64,
    /// Bases brought back by [`ImageRef::rematerialise`](crate::ImageRef::rematerialise),
    /// off the render thread. They are counted in `from_spill` /
    /// `from_source` too.
    pub rematerialised: u64,
}

#[derive(Debug, Default)]
struct Counters {
    evictions: AtomicU64,
    evicted_bytes: AtomicU64,
    from_source: AtomicU64,
    from_spill: AtomicU64,
    levels_rebuilt: AtomicU64,
    spill_writes: AtomicU64,
    spill_bytes: AtomicU64,
    spill_failures: AtomicU64,
    lost: AtomicU64,
    substituted: AtomicU64,
    rematerialised: AtomicU64,
}

#[derive(Debug)]
struct Entry {
    store: Weak<ImageStore>,
    evictable: u64,
    proxy: u64,
    last_use: u64,
}

#[derive(Debug, Default)]
struct Ledger {
    entries: HashMap<u64, Entry>,
    tick: u64,
    evictable: u64,
    proxy: u64,
    peak: u64,
}

/// A pixel memory budget shared by every image registered with it.
pub struct PixelBudget {
    limit: AtomicU64,
    proxy_cap: u32,
    proxy_default: u32,
    spill_root: Option<PathBuf>,
    spill_enabled: AtomicBool,
    spill_dir: Mutex<Option<Arc<SpillDir>>>,
    ledger: Mutex<Ledger>,
    next_key: AtomicU64,
    counters: Counters,
}

impl fmt::Debug for PixelBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixelBudget")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

static GLOBAL: LazyLock<Arc<PixelBudget>> =
    LazyLock::new(|| PixelBudget::new(BudgetConfig::from_env()));

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding a store or ledger lock leaves plain data that
    // is still consistent enough to count and free; never propagate it.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl PixelBudget {
    /// A budget of its own, for a test, a tool or an export.
    #[must_use]
    pub fn new(config: BudgetConfig) -> Arc<PixelBudget> {
        Arc::new(PixelBudget {
            limit: AtomicU64::new(config.limit_bytes),
            proxy_cap: config.proxy_cap.max(1),
            proxy_default: config.proxy_default.max(1),
            spill_root: config.spill_root,
            spill_enabled: AtomicBool::new(config.spill),
            spill_dir: Mutex::new(None),
            ledger: Mutex::new(Ledger::default()),
            next_key: AtomicU64::new(0),
            counters: Counters::default(),
        })
    }

    /// The process-wide budget [`ImageRef::new`](crate::ImageRef::new)
    /// registers with, configured by [`BudgetConfig::from_env`].
    #[must_use]
    pub fn global() -> &'static Arc<PixelBudget> {
        &GLOBAL
    }

    /// The limit on evictable bytes.
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit.load(Ordering::Relaxed)
    }

    /// Changes the limit and evicts down to it.
    pub fn set_limit(&self, bytes: u64) {
        self.limit.store(bytes, Ordering::Relaxed);
        self.enforce();
    }

    /// A snapshot of the accounting and the counters.
    #[must_use]
    pub fn stats(&self) -> BudgetStats {
        let l = lock(&self.ledger);
        let c = &self.counters;
        BudgetStats {
            limit_bytes: self.limit(),
            evictable_bytes: l.evictable,
            proxy_bytes: l.proxy,
            peak_bytes: l.peak,
            images: l.entries.len() as u64,
            evictions: c.evictions.load(Ordering::Relaxed),
            evicted_bytes: c.evicted_bytes.load(Ordering::Relaxed),
            from_source: c.from_source.load(Ordering::Relaxed),
            from_spill: c.from_spill.load(Ordering::Relaxed),
            levels_rebuilt: c.levels_rebuilt.load(Ordering::Relaxed),
            spill_writes: c.spill_writes.load(Ordering::Relaxed),
            spill_bytes: c.spill_bytes.load(Ordering::Relaxed),
            spill_failures: c.spill_failures.load(Ordering::Relaxed),
            lost: c.lost.load(Ordering::Relaxed),
            substituted: c.substituted.load(Ordering::Relaxed),
            rematerialised: c.rematerialised.load(Ordering::Relaxed),
        }
    }

    /// The spill session directory, if one has been created.
    #[must_use]
    pub fn spill_path(&self) -> Option<PathBuf> {
        lock(&self.spill_dir)
            .as_ref()
            .map(|d| d.path().to_path_buf())
    }

    /// Evicts least recently sampled stores until the evictable bytes are
    /// within the limit, or nothing more can be evicted.
    pub fn enforce(&self) {
        let mut tried: HashSet<u64> = HashSet::new();
        loop {
            let victim = {
                let l = lock(&self.ledger);
                if l.evictable <= self.limit() {
                    return;
                }
                l.entries
                    .iter()
                    .filter(|(k, e)| e.evictable > 0 && !tried.contains(*k))
                    .min_by_key(|(k, e)| (e.last_use, **k))
                    .map(|(k, e)| (*k, e.store.clone()))
            };
            let Some((key, store)) = victim else { return };
            tried.insert(key);
            if let Some(store) = store.upgrade() {
                store.evict();
            }
        }
    }

    fn register(&self, key: u64, store: Weak<ImageStore>) {
        let mut l = lock(&self.ledger);
        l.tick += 1;
        let last_use = l.tick;
        l.entries.insert(
            key,
            Entry {
                store,
                evictable: 0,
                proxy: 0,
                last_use,
            },
        );
    }

    fn unregister(&self, key: u64) {
        let mut l = lock(&self.ledger);
        if let Some(e) = l.entries.remove(&key) {
            l.evictable -= e.evictable;
            l.proxy -= e.proxy;
        }
    }

    /// Records a store's resident bytes; `touch` marks it just used.
    fn account(&self, key: u64, evictable: u64, proxy: u64, touch: bool) {
        let mut l = lock(&self.ledger);
        l.tick += 1;
        let tick = l.tick;
        let Some(e) = l.entries.get_mut(&key) else {
            return;
        };
        let (old_e, old_p) = (e.evictable, e.proxy);
        e.evictable = evictable;
        e.proxy = proxy;
        if touch {
            e.last_use = tick;
        }
        l.evictable = l.evictable - old_e + evictable;
        l.proxy = l.proxy - old_p + proxy;
        l.peak = l.peak.max(l.evictable + l.proxy);
    }

    fn spill(&self, data: &[u8]) -> Option<SpillFile> {
        if !self.spill_enabled.load(Ordering::Relaxed) {
            return None;
        }
        let dir = {
            let mut d = lock(&self.spill_dir);
            if d.is_none() {
                let root = self
                    .spill_root
                    .clone()
                    .unwrap_or_else(crate::spill::default_root);
                match SpillDir::create(&root) {
                    Ok(dir) => *d = Some(Arc::new(dir)),
                    Err(_) => {
                        // No directory, no spilling for this budget.
                        self.spill_enabled.store(false, Ordering::Relaxed);
                        self.counters.spill_failures.fetch_add(1, Ordering::Relaxed);
                        return None;
                    }
                }
            }
            d.clone()?
        };
        match dir.write(data) {
            Ok(f) => {
                self.counters.spill_writes.fetch_add(1, Ordering::Relaxed);
                self.counters
                    .spill_bytes
                    .fetch_add(data.len() as u64, Ordering::Relaxed);
                Some(f)
            }
            Err(_) => {
                self.counters.spill_failures.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
}

/// What [`ImageStore::pin`] hands a sampler.
#[derive(Debug)]
pub(crate) enum Pinned {
    /// Every level asked for, in order.
    Levels(Vec<LevelBuf>),
    /// The levels asked for need the base back and the caller would not
    /// wait: the best resident level instead, with its index.
    Substitute(usize, LevelBuf),
}

/// One level resident in memory, shared with whoever pinned it.
#[derive(Debug, Clone)]
pub struct LevelBuf {
    /// Width in texels, at least 1 for a non-empty image.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// `width · height · 4` bytes of straight sRGB RGBA8, top row first.
    pub data: Arc<Vec<u8>>,
}

impl LevelBuf {
    /// Borrows it as a [`Level`](crate::resample::Level).
    #[must_use]
    pub fn as_level(&self) -> crate::resample::Level<'_> {
        crate::resample::Level {
            width: self.width,
            height: self.height,
            data: &self.data,
        }
    }

    /// One texel, with the repeat mode applied to both axes.
    #[must_use]
    pub fn texel(&self, x: i64, y: i64, repeat: crate::paint::Repeat) -> xarast_color::Rgba8 {
        self.as_level().texel(x, y, repeat)
    }
}

#[derive(Debug)]
struct StoreState {
    levels: Vec<Option<Arc<Vec<u8>>>>,
    /// Levels at this index and above are never evicted.
    proxy: usize,
    /// Which levels have ever been built, to tell a rebuild from a build.
    built: Vec<bool>,
    spilled: Option<SpillFile>,
}

/// The levels of one image and where its base comes back from. What an
/// [`ImageRef`](crate::ImageRef) shares between its clones.
pub(crate) struct ImageStore {
    dims: Vec<(u32, u32)>,
    budget: Arc<PixelBudget>,
    key: u64,
    source: Option<Arc<dyn PixelSource>>,
    state: Mutex<StoreState>,
    /// The [`substitution_tick`] of its last substitution, 0 for never.
    substituted_at: AtomicU64,
}

impl fmt::Debug for ImageStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageStore")
            .field("dims", &self.dims.first())
            .field("levels", &self.dims.len())
            .field("source", &self.source.is_some())
            .finish_non_exhaustive()
    }
}

/// The dimensions of every level, base first, down to 1 × 1.
fn level_dims(width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut out = vec![(width, height)];
    let (mut w, mut h) = (width, height);
    if w == 0 || h == 0 {
        return out;
    }
    while w > 1 || h > 1 {
        (w, h) = (w.div_ceil(2), h.div_ceil(2));
        out.push((w, h));
    }
    out
}

/// The first (largest) level whose long edge is at most `edge`.
fn first_level_within(dims: &[(u32, u32)], edge: u32) -> usize {
    dims.iter()
        .position(|&(w, h)| w.max(h) <= edge)
        .unwrap_or(dims.len().saturating_sub(1))
}

impl ImageStore {
    pub(crate) fn new(
        width: u32,
        height: u32,
        data: Vec<u8>,
        budget: &Arc<PixelBudget>,
        source: Option<Arc<dyn PixelSource>>,
    ) -> Arc<ImageStore> {
        let dims = level_dims(width, height);
        let proxy = first_level_within(&dims, budget.proxy_default);
        let mut levels = vec![None; dims.len()];
        levels[0] = Some(Arc::new(data));
        let mut built = vec![false; dims.len()];
        built[0] = true;
        let key = budget.next_key.fetch_add(1, Ordering::Relaxed);
        let store = Arc::new(ImageStore {
            dims,
            budget: Arc::clone(budget),
            key,
            source,
            state: Mutex::new(StoreState {
                levels,
                proxy,
                built,
                spilled: None,
            }),
            substituted_at: AtomicU64::new(0),
        });
        budget.register(key, Arc::downgrade(&store));
        {
            let st = lock(&store.state);
            store.recount(&st, true);
        }
        budget.enforce();
        store
    }

    pub(crate) fn level_count(&self) -> usize {
        self.dims.len()
    }

    pub(crate) fn has_pyramid(&self) -> bool {
        lock(&self.state).built.iter().skip(1).any(|&b| b)
    }

    /// The proxy index now.
    pub(crate) fn proxy_level(&self) -> usize {
        lock(&self.state).proxy
    }

    /// Which levels are resident now.
    pub(crate) fn resident(&self) -> Vec<bool> {
        lock(&self.state)
            .levels
            .iter()
            .map(Option::is_some)
            .collect()
    }

    /// Pins levels `lo..=hi` (clamped to the pyramid), re-producing any
    /// that are not resident. `draw` records that the image is drawn at
    /// `lo`, which may grow the proxy; a plain read (equality, a test
    /// looking at a level) does not. The result has one entry per level in
    /// the range.
    pub(crate) fn pin(&self, lo: usize, hi: usize, draw: bool) -> Vec<LevelBuf> {
        match self.pin_with(lo, hi, draw, MissingLevels::Materialise) {
            Pinned::Levels(v) => v,
            // `Materialise` never substitutes.
            Pinned::Substitute(_, l) => vec![l],
        }
    }

    /// [`ImageStore::pin`] under a policy for levels that need the base
    /// back (`MissingLevels`).
    pub(crate) fn pin_with(
        &self,
        lo: usize,
        hi: usize,
        draw: bool,
        missing: MissingLevels,
    ) -> Pinned {
        let last = self.dims.len() - 1;
        let (lo, hi) = (lo.min(last), hi.min(last).max(lo.min(last)));
        let out = {
            let mut st = lock(&self.state);
            let cap = first_level_within(&self.dims, self.budget.proxy_cap);
            let wanted = lo.max(cap);
            if draw && wanted < st.proxy {
                st.proxy = wanted;
            }
            // `lo` needs the base back when neither it nor any larger level
            // is resident; every level above it is then a reduction of it.
            let needs_base = (0..=lo).all(|j| st.levels[j].is_none());
            let substitute = (missing == MissingLevels::Substitute && needs_base)
                .then(|| (lo + 1..=last).find(|&j| st.levels[j].is_some()))
                .flatten();
            let out = match substitute {
                Some(j) => {
                    let (width, height) = self.dims[j];
                    let data = st.levels[j].clone().unwrap_or_default();
                    self.budget
                        .counters
                        .substituted
                        .fetch_add(1, Ordering::Relaxed);
                    let tick = SUBSTITUTIONS.fetch_add(1, Ordering::SeqCst) + 1;
                    self.substituted_at.fetch_max(tick, Ordering::SeqCst);
                    Pinned::Substitute(
                        j,
                        LevelBuf {
                            width,
                            height,
                            data,
                        },
                    )
                }
                None => Pinned::Levels((lo..=hi).map(|i| self.ensure(&mut st, i)).collect()),
            };
            self.recount(&st, true);
            out
        };
        self.budget.enforce();
        out
    }

    /// Whether a sampler drew a substitute for this image after `tick`
    /// ([`substitution_tick`]).
    pub(crate) fn substituted_since(&self, tick: u64) -> bool {
        self.substituted_at.load(Ordering::SeqCst) > tick
    }

    /// Makes the base resident again (a spill read or a decode), for a
    /// caller that drew a substitute and does this off its render thread.
    /// Returns whether anything had to be brought back.
    pub(crate) fn rematerialise(&self) -> bool {
        let brought = {
            let mut st = lock(&self.state);
            if st.levels[0].is_some() {
                false
            } else {
                let _ = self.ensure(&mut st, 0);
                self.recount(&st, true);
                true
            }
        };
        if brought {
            self.budget
                .counters
                .rematerialised
                .fetch_add(1, Ordering::Relaxed);
            self.budget.enforce();
        }
        brought
    }

    /// Builds every level (off the render thread, next to the decode).
    pub(crate) fn prepare(&self) {
        {
            let mut st = lock(&self.state);
            let last = self.dims.len() - 1;
            if last > 0 && st.levels[last].is_none() {
                let _ = self.ensure(&mut st, last);
            }
            self.recount(&st, false);
        }
        self.budget.enforce();
    }

    /// Level `i`, made resident. Called with the state locked.
    fn ensure(&self, st: &mut StoreState, i: usize) -> LevelBuf {
        let (width, height) = self.dims[i];
        if let Some(d) = &st.levels[i] {
            return LevelBuf {
                width,
                height,
                data: Arc::clone(d),
            };
        }
        if i == 0 {
            let data = Arc::new(self.materialise_base(st));
            st.levels[0] = Some(Arc::clone(&data));
            return LevelBuf {
                width,
                height,
                data,
            };
        }
        // The nearest larger level that is resident, or the base.
        let from = (1..i).rev().find(|&j| st.levels[j].is_some()).unwrap_or(0);
        let mut src = self.ensure(st, from);
        for k in from + 1..=i {
            let (w, h) = self.dims[k - 1];
            let next = Arc::new(crate::resample::reduce_level(w, h, &src.data).2);
            st.levels[k] = Some(Arc::clone(&next));
            if st.built[k] {
                self.budget
                    .counters
                    .levels_rebuilt
                    .fetch_add(1, Ordering::Relaxed);
            }
            st.built[k] = true;
            let (width, height) = self.dims[k];
            src = LevelBuf {
                width,
                height,
                data: next,
            };
        }
        src
    }

    fn materialise_base(&self, st: &StoreState) -> Vec<u8> {
        let (w, h) = self.dims[0];
        let len = w as usize * h as usize * 4;
        let c = &self.budget.counters;
        if let Some(f) = &st.spilled {
            match f.read() {
                Ok(d) if d.len() == len => {
                    c.from_spill.fetch_add(1, Ordering::Relaxed);
                    return d;
                }
                _ => {
                    c.spill_failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if let Some(d) = self.source.as_ref().and_then(|s| s.materialise())
            && d.len() == len
        {
            c.from_source.fetch_add(1, Ordering::Relaxed);
            return d;
        }
        // Unreachable unless a spill file was damaged and there is no
        // source: eviction never drops a base it cannot bring back.
        c.lost.fetch_add(1, Ordering::Relaxed);
        vec![0; len]
    }

    /// Drops the evictable levels, spilling the base first when that is
    /// the cheaper way back.
    fn evict(&self) {
        let freed = {
            let mut st = lock(&self.state);
            let before = self.bytes(&st).0;
            if before == 0 {
                return;
            }
            let p = st.proxy;
            let mut drop_base = p > 0;
            if drop_base && st.spilled.is_none() {
                let cheap = self.source.as_ref().is_some_and(|s| s.is_cheap());
                if !cheap && let Some(base) = &st.levels[0] {
                    st.spilled = self.budget.spill(base);
                }
                drop_base = cheap || st.spilled.is_some() || self.source.is_some();
            }
            // What a sampler that will not wait draws instead
            // (`MissingLevels::Substitute`): some level at or below the
            // proxy stays resident whenever the base goes.
            if drop_base
                && p < self.dims.len()
                && st.levels[0].is_some()
                && st.levels[p..].iter().all(Option::is_none)
            {
                let _ = self.ensure(&mut st, p);
            }
            let first = usize::from(!drop_base);
            for slot in st.levels.iter_mut().take(p).skip(first) {
                *slot = None;
            }
            self.recount(&st, false);
            before - self.bytes(&st).0
        };
        if freed > 0 {
            let c = &self.budget.counters;
            c.evictions.fetch_add(1, Ordering::Relaxed);
            c.evicted_bytes.fetch_add(freed, Ordering::Relaxed);
        }
    }

    /// Resident (evictable, proxy) bytes.
    fn bytes(&self, st: &StoreState) -> (u64, u64) {
        let mut e = 0u64;
        let mut p = 0u64;
        for (i, l) in st.levels.iter().enumerate() {
            if let Some(d) = l {
                if i < st.proxy {
                    e += d.len() as u64;
                } else {
                    p += d.len() as u64;
                }
            }
        }
        (e, p)
    }

    fn recount(&self, st: &StoreState, touch: bool) {
        let (e, p) = self.bytes(st);
        self.budget.account(self.key, e, p, touch);
    }
}

impl Drop for ImageStore {
    fn drop(&mut self) {
        self.budget.unregister(self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_dims_halve_down_to_one_by_one() {
        assert_eq!(level_dims(5, 3), vec![(5, 3), (3, 2), (2, 1), (1, 1)]);
        assert_eq!(level_dims(1, 1), vec![(1, 1)]);
        assert_eq!(level_dims(0, 4), vec![(0, 4)]);
        let d = level_dims(6000, 4000);
        assert_eq!(first_level_within(&d, 2048), 2);
        assert_eq!(d[2], (1500, 1000));
        assert_eq!(first_level_within(&d, 256), 5);
        assert_eq!(first_level_within(&level_dims(100, 50), 2048), 0);
    }
}
