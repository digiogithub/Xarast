//! Deferred images (XARA-T-0304): an image whose base is produced by its
//! source when first needed. Under `Materialise` it is the eager image
//! byte for byte; under `Substitute` it draws its stand-in until
//! `rematerialise` makes the base and the whole pyramid, off the render
//! thread and without holding the image while it works; and comparing
//! one never produces it.
//!
//! Every test makes its own [`PixelBudget`].

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use xarast_render::{
    BudgetConfig, CpuBackend, CpuConfig, DirtyRect, DisplayList, Filter, FnSource, GradMapping,
    ImageRef, LevelBuf, MissingLevels, PixelBudget, PixelSource, Point64, RenderQuality, Repeat,
    Scene, SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams, substitution_tick,
};

fn noisy(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761) | 1;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let b = x.to_le_bytes();
        out.extend_from_slice(&[b[0], b[1], b[2], b[3] | 0x40]);
    }
    out
}

fn budget() -> Arc<PixelBudget> {
    PixelBudget::new(BudgetConfig {
        spill: false,
        ..BudgetConfig::unlimited()
    })
}

/// A flat grey stand-in of `w` × `h`, claiming level `level`.
fn standin(level: usize, w: u32, h: u32) -> (usize, LevelBuf) {
    (
        level,
        LevelBuf {
            width: w,
            height: h,
            data: Arc::new([128, 128, 128, 255].repeat((w * h) as usize)),
        },
    )
}

/// A source that hands out `data` and counts its calls.
fn counted(data: Vec<u8>) -> (Arc<dyn PixelSource>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    let source: Arc<dyn PixelSource> = Arc::new(FnSource::expensive(move || {
        c.fetch_add(1, Ordering::SeqCst);
        Some(data.clone())
    }));
    (source, calls)
}

/// Draws `image` on a `side`-pixel square with `filter` under `missing`.
fn draw(image: &ImageRef, side: f64, filter: Filter, missing: MissingLevels) -> Surface {
    let mut res = xarast_render::Resolver::new();
    let id = res.images.insert(image.clone());
    let mapping = GradMapping::Affine {
        a: Point64::new(0.0, 0.0),
        b: Point64::new(0.0, side),
        c: Point64::new(side, 0.0),
    };
    let mut scene = Scene::new();
    {
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        let paint = xarast_render::Paint::Image {
            image: id,
            mapping,
            repeat: Repeat::Simple,
            filter,
            contone: None,
            adjust: xarast_render::BitmapAdjust::default(),
        };
        b.image(SceneNodeId(0), id, mapping, paint);
        b.finish().expect("balanced");
    }
    let n = side.ceil() as u32;
    let v = ViewParams::new(n, n, Transform2D::IDENTITY, RenderQuality::Final);
    let dl = DisplayList::build(&scene, &v, &DirtyRect::NONE);
    let mut target = Surface::new(n, n);
    CpuBackend::new(CpuConfig {
        missing_levels: missing,
        ..CpuConfig::deterministic()
    })
    .render(&dl, &res, &mut target)
    .expect("renders");
    target
}

const SIDES: [f64; 4] = [37.0, 180.0, 640.0, 1100.0];
const FILTERS: [Filter; 2] = [Filter::Nearest, Filter::Bilinear];

#[test]
fn under_materialise_a_deferred_image_is_the_eager_one_byte_for_byte() {
    let (w, h) = (700, 500);
    let data = noisy(w, h, 7);
    let b = budget();
    let eager = ImageRef::with_budget(w, h, data.clone(), &b, None);
    eager.prepare();
    let (source, calls) = counted(data);
    let deferred = ImageRef::deferred(w, h, &b, source, Some(standin(2, 175, 125)));
    assert!(deferred.is_pending());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "registration makes nothing"
    );
    for side in SIDES {
        for filter in FILTERS {
            let want = draw(&eager, side, filter, MissingLevels::Materialise);
            let got = draw(&deferred, side, filter, MissingLevels::Materialise);
            assert!(
                got.data() == want.data(),
                "side {side}, {filter:?}: the stand-in leaked into a Materialise render"
            );
        }
    }
    assert!(!deferred.is_pending());
    assert_eq!(calls.load(Ordering::SeqCst), 1, "made once");
    for i in 0..eager.level_count() {
        assert_eq!(eager.level(i).data, deferred.level(i).data, "level {i}");
    }
}

#[test]
fn substitute_draws_the_stand_in_until_rematerialise_makes_every_level() {
    let (w, h) = (640, 480);
    let data = noisy(w, h, 11);
    let b = budget();
    let eager = ImageRef::with_budget(w, h, data.clone(), &b, None);
    eager.prepare();
    let (source, calls) = counted(data);
    let deferred = ImageRef::deferred(w, h, &b, source, Some(standin(2, 160, 120)));

    for side in SIDES {
        let tick = substitution_tick();
        let early = draw(&deferred, side, Filter::Bilinear, MissingLevels::Substitute);
        assert!(deferred.substituted_since(tick), "side {side}");
        assert!(deferred.is_pending(), "a Substitute render made nothing");
        let want = draw(&eager, side, Filter::Bilinear, MissingLevels::Materialise);
        assert!(
            early.data() != want.data(),
            "side {side}: grey is not the photo"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        b.stats().substituted >= SIDES.len() as u64,
        "one per sampler"
    );

    assert!(deferred.rematerialise());
    assert!(!deferred.is_pending());
    assert!(
        deferred.resident_levels().iter().all(|&r| r),
        "the whole pyramid came with the base"
    );
    assert!(!deferred.rematerialise(), "nothing left to make");
    let s = b.stats();
    assert_eq!((s.rematerialised, s.from_source), (1, 1), "{s:?}");
    for side in SIDES {
        for filter in FILTERS {
            let tick = substitution_tick();
            let got = draw(&deferred, side, filter, MissingLevels::Substitute);
            assert!(!deferred.substituted_since(tick));
            let want = draw(&eager, side, filter, MissingLevels::Materialise);
            assert!(got.data() == want.data(), "side {side}, {filter:?}");
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The accounting forgot the stand-in: the budget holds the levels
    // of two identical images.
    let s = b.stats();
    assert_eq!(s.evictable_bytes + s.proxy_bytes, 2 * level_bytes(&eager));
}

fn level_bytes(image: &ImageRef) -> u64 {
    (0..image.level_count())
        .map(|i| image.level(i).data.len() as u64)
        .sum()
}

/// The helper makes a base with the image unlocked: a render that pins
/// the image meanwhile draws the stand-in at once instead of waiting
/// behind the evaluation.
#[test]
fn rematerialise_does_not_hold_the_image_while_it_makes_it() {
    let (w, h) = (300, 200);
    let data = noisy(w, h, 3);
    let b = budget();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(entered_tx);
    let go_rx = Mutex::new(go_rx);
    let source: Arc<dyn PixelSource> = Arc::new(FnSource::expensive(move || {
        let _ = entered_tx.lock().map(|t| t.send(()));
        let _ = go_rx
            .lock()
            .map(|r| r.recv_timeout(Duration::from_secs(60)));
        Some(data.clone())
    }));
    let deferred = ImageRef::deferred(w, h, &b, source, Some(standin(1, 150, 100)));
    let helper = {
        let image = deferred.clone();
        std::thread::spawn(move || image.rematerialise())
    };
    entered_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the helper started");
    // The source is blocked: this render must not be.
    let (done_tx, done_rx) = mpsc::channel();
    {
        let image = deferred.clone();
        std::thread::spawn(move || {
            let _ = draw(&image, 100.0, Filter::Bilinear, MissingLevels::Substitute);
            let _ = done_tx.send(image.is_pending());
        });
    }
    let pending = done_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the render waited for the helper's evaluation");
    assert!(pending, "drawn from the stand-in while the helper works");
    go_tx.send(()).unwrap();
    assert!(helper.join().unwrap());
    assert!(!deferred.is_pending());
}

#[test]
fn comparing_never_produces_a_deferred_base() {
    let (w, h) = (64, 48);
    let data = noisy(w, h, 5);
    let b = budget();
    let (sa, ca) = counted(data.clone());
    let (sb, cb) = counted(data.clone());
    let a = ImageRef::deferred(w, h, &b, sa, Some(standin(1, 32, 24)));
    let c = ImageRef::deferred(w, h, &b, sb, Some(standin(1, 32, 24)));
    let eager = ImageRef::with_budget(w, h, data, &b, None);
    assert!(a.eq_without_producing(&a.clone()), "the same store");
    assert!(!a.eq_without_producing(&c), "unknown is different");
    assert!(!a.eq_without_producing(&eager));
    assert!(a.is_pending() && c.is_pending());
    assert_eq!(ca.load(Ordering::SeqCst) + cb.load(Ordering::SeqCst), 0);
    // Once made, they compare by content like any image.
    a.rematerialise();
    c.rematerialise();
    assert!(a.eq_without_producing(&c) && a.eq_without_producing(&eager));
}
