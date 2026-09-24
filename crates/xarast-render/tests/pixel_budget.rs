//! The pixel memory budget (W10.5): eviction, spilling and
//! re-materialisation never change a rendered pixel, the proxy is never
//! evicted, and the accounting balances.
//!
//! Every test makes its own [`PixelBudget`], so nothing here depends on
//! the process-wide one or on the other tests running in parallel.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::render_case_with;
use xarast_render::corpus::{Case, all_cases};
use xarast_render::{
    BudgetConfig, CpuBackend, CpuConfig, DirtyRect, DisplayList, Filter, FnSource, GradMapping,
    ImageRef, ImageRegistry, Paint, PixelBudget, PixelSource, Point64, RenderQuality, Repeat,
    Scene, SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams,
};

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("xarast-budget-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

/// A budget that evicts everything it can: no evictable byte may stay
/// resident, and every level above the 1 × 1 one is evictable.
fn tiny(root: &std::path::Path) -> Arc<PixelBudget> {
    PixelBudget::new(BudgetConfig {
        limit_bytes: 0,
        proxy_cap: 1,
        proxy_default: 1,
        spill_root: Some(root.to_path_buf()),
        spill: true,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    /// No source: an evicted base is spilled and read back.
    Spill,
    /// A source that costs a decode: spilled too.
    Expensive,
    /// A source that costs a copy: dropped and copied back.
    Cheap,
}

fn rewrap(image: &ImageRef, budget: &Arc<PixelBudget>, way: Way) -> ImageRef {
    let (w, h) = (image.width(), image.height());
    let pixels: Arc<Vec<u8>> = Arc::new(image.level(0).data.to_vec());
    let source: Option<Arc<dyn PixelSource>> = match way {
        Way::Spill => None,
        Way::Expensive => {
            let p = Arc::clone(&pixels);
            Some(Arc::new(FnSource::expensive(move || Some(p.to_vec()))))
        }
        Way::Cheap => {
            let p = Arc::clone(&pixels);
            Some(Arc::new(FnSource::cheap(move || Some(p.to_vec()))))
        }
    };
    ImageRef::with_budget(w, h, pixels.to_vec(), budget, source)
}

fn rebudget(case: &Case, budget: &Arc<PixelBudget>, way: Way, prepare: bool) -> Case {
    let mut resolver = case.resolver.clone();
    resolver.images = ImageRegistry::new();
    for image in case.resolver.images.iter() {
        let image = rewrap(image, budget, way);
        if prepare {
            image.prepare();
        }
        resolver.images.insert(image);
    }
    Case {
        name: case.name.clone(),
        scene: case.scene.clone(),
        resolver,
        view: case.view,
    }
}

#[test]
fn every_image_case_renders_byte_for_byte_under_a_tiny_budget() {
    let cases: Vec<Case> = all_cases()
        .into_iter()
        .filter(|c| !c.resolver.images.is_empty())
        .collect();
    assert!(
        cases.len() >= 10,
        "the feature corpus has {} image cases",
        cases.len()
    );
    for way in [Way::Spill, Way::Expensive, Way::Cheap] {
        for prepare in [false, true] {
            let root = scratch(&format!("{way:?}-{prepare}"));
            let budget = tiny(&root);
            for case in &cases {
                let reference = render_case_with(case, CpuConfig::deterministic());
                let squeezed = rebudget(case, &budget, way, prepare);
                for cfg in [CpuConfig::deterministic(), CpuConfig::interactive()] {
                    let got = render_case_with(&squeezed, cfg);
                    assert!(
                        got.data() == reference.data(),
                        "{} ({way:?}, prepare {prepare}) differs under a tiny budget",
                        case.name
                    );
                }
            }
            let s = budget.stats();
            assert_eq!(s.evictable_bytes, 0, "{way:?}: {s:?}");
            assert!(s.evictions > 0, "{way:?}: {s:?}");
            assert_eq!(s.lost, 0, "{way:?}: {s:?}");
            assert_eq!(s.spill_failures, 0, "{way:?}: {s:?}");
            match way {
                Way::Spill | Way::Expensive => {
                    assert!(s.spill_writes > 0 && s.from_spill > 0, "{way:?}: {s:?}");
                    assert_eq!(s.from_source, 0, "{way:?}: {s:?}");
                }
                Way::Cheap => {
                    assert!(s.from_source > 0, "{way:?}: {s:?}");
                    assert_eq!(s.spill_writes, 0, "{way:?}: {s:?}");
                }
            }
            if prepare {
                assert!(s.levels_rebuilt > 0, "{way:?}: {s:?}");
            }
            drop(budget);
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}

/// A `w × h` image with detail everywhere.
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

/// Draws `image` placed on a `side`-pixel square at `filter`.
fn draw(image: &ImageRef, side: f64, filter: Filter) -> Surface {
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
        let paint = Paint::Image {
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
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &res, &mut target)
        .expect("renders");
    target
}

#[test]
fn the_proxy_is_never_evicted_and_a_zoomed_out_view_needs_nothing_else() {
    let root = scratch("proxy");
    let budget = PixelBudget::new(BudgetConfig {
        limit_bytes: 0,
        spill_root: Some(root.clone()),
        ..BudgetConfig::unlimited()
    });
    // 1024 × 512: levels 1024, 512, 256 (the default proxy), 128, …
    let image = ImageRef::with_budget(1024, 512, noisy(1024, 512, 1), &budget, None);
    image.prepare();
    assert_eq!(image.proxy_level(), 2);
    let resident = image.resident_levels();
    assert_eq!(&resident[..2], &[false, false], "base and level 1 evicted");
    assert!(resident[2..].iter().all(|&r| r), "the proxy and below stay");
    let s = budget.stats();
    assert_eq!(s.evictable_bytes, 0);
    assert_eq!(s.spill_writes, 1, "the base is spilled once");
    let proxy_bytes: u64 = (2..image.level_count())
        .map(|i| image.level(i).data.len() as u64)
        .sum();
    assert_eq!(s.proxy_bytes, proxy_bytes);

    // 1024 texels on 100 px: more than 8 texels per pixel, levels 3 and
    // 4 — inside the proxy. Nothing is read back or rebuilt.
    let before = budget.stats();
    let _ = draw(&image, 100.0, Filter::HighQuality);
    let after = budget.stats();
    assert_eq!(after.from_spill, before.from_spill);
    assert_eq!(after.levels_rebuilt, before.levels_rebuilt);
    assert_eq!(image.proxy_level(), 2, "a smaller draw never shrinks it");

    // Drawn at its own size, a 1024-texel image is under the 2048 cap:
    // the base becomes the proxy and stays resident from now on.
    let _ = draw(&image, 1024.0, Filter::HighQuality);
    assert_eq!(image.proxy_level(), 0);
    assert!(image.resident_levels()[0]);
    assert_eq!(budget.stats().evictable_bytes, 0);
    assert!(budget.stats().from_spill >= 1);
    drop(image);
    drop(budget);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_proxy_is_capped_on_the_long_edge() {
    let root = scratch("cap");
    let budget = PixelBudget::new(BudgetConfig {
        limit_bytes: 0,
        proxy_cap: 64,
        proxy_default: 16,
        spill_root: Some(root.clone()),
        spill: true,
    });
    let data = noisy(256, 128, 2);
    let image = ImageRef::with_budget(256, 128, data.clone(), &budget, None);
    // Magnified: the draw asks for the base, but the proxy stops at the
    // 64-texel level (256 → 128 → 64).
    let a = draw(&image, 300.0, Filter::HighQuality);
    assert_eq!(image.proxy_level(), 2);
    assert_eq!(&image.resident_levels()[..2], &[false, false]);
    // And the picture is the one an unbudgeted image gives.
    let free = ImageRef::with_budget(
        256,
        128,
        data,
        &PixelBudget::new(BudgetConfig::unlimited()),
        None,
    );
    let b = draw(&free, 300.0, Filter::HighQuality);
    assert!(a.data() == b.data());
    drop(image);
    drop(budget);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_accounting_balances_and_spill_files_go_with_their_images() {
    let root = scratch("accounting");
    let budget = PixelBudget::new(BudgetConfig {
        limit_bytes: u64::MAX,
        spill_root: Some(root.clone()),
        ..BudgetConfig::unlimited()
    });
    let a = ImageRef::with_budget(300, 200, noisy(300, 200, 3), &budget, None);
    let b = ImageRef::with_budget(40, 30, noisy(40, 30, 4), &budget, None);
    a.prepare();
    let s = budget.stats();
    assert_eq!(s.images, 2);
    // Counted from the dimensions: sampling a level would count as a draw
    // and move the proxy.
    let mut total = 40 * 30 * 4;
    let (mut w, mut h) = (300u64, 200u64);
    loop {
        total += w * h * 4;
        if w == 1 && h == 1 {
            break;
        }
        (w, h) = (w.div_ceil(2), h.div_ceil(2));
    }
    assert_eq!(a.proxy_level(), 1);
    assert_eq!(s.evictable_bytes + s.proxy_bytes, total);
    assert_eq!(s.evictions, 0, "an unlimited budget never evicts");

    // Squeeze: `a` spills its base (300 × 200 is above the 256 default
    // proxy); `b` is its own proxy and cannot be evicted.
    budget.set_limit(0);
    let s = budget.stats();
    assert_eq!(s.evictable_bytes, 0);
    assert_eq!(s.spill_writes, 1);
    let session = budget.spill_path().expect("a spill directory exists");
    let files = || {
        std::fs::read_dir(&session)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".rgba"))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(files(), 1);
    // A clone shares the store: dropping one keeps the spill file.
    let a2 = a.clone();
    drop(a);
    assert_eq!(files(), 1);
    assert_eq!(a2.level(0).data.len(), 300 * 200 * 4);
    drop(a2);
    assert_eq!(files(), 0, "the spill file goes with its image");
    drop(b);
    let s = budget.stats();
    assert_eq!((s.images, s.evictable_bytes, s.proxy_bytes), (0, 0, 0));
    drop(budget);
    assert!(
        !session.exists(),
        "the session directory goes with the budget"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn equality_survives_eviction() {
    let root = scratch("eq");
    let budget = tiny(&root);
    let data = noisy(64, 64, 5);
    let a = ImageRef::with_budget(64, 64, data.clone(), &budget, None);
    let b = ImageRef::with_budget(64, 64, data, &budget, None);
    assert!(!a.resident_levels()[0]);
    assert_eq!(a, b);
    let c = ImageRef::with_budget(64, 64, noisy(64, 64, 6), &budget, None);
    assert_ne!(a, c);
    drop((a, b, c));
    drop(budget);
    let _ = std::fs::remove_dir_all(&root);
}

/// Four photographs placed on a 320 × 240 view at `scale` device pixels
/// per texel, drawn at `quality`'s filter.
fn photos(
    images: &[ImageRef],
    scale: f64,
    quality: RenderQuality,
) -> (Scene, xarast_render::Resolver, ViewParams) {
    let filter = match quality {
        RenderQuality::Draft => Filter::Nearest,
        RenderQuality::Final => Filter::HighQuality,
    };
    let mut res = xarast_render::Resolver::new();
    let mut scene = Scene::new();
    {
        let mut b = SceneBuilder::begin(&mut scene, quality);
        for (k, image) in images.iter().enumerate() {
            let id = res.images.insert(image.clone());
            let (w, h) = (
                f64::from(image.width()) * scale,
                f64::from(image.height()) * scale,
            );
            let (x, y) = (
                5.0 + 160.0 * f64::from(k as u32 % 2),
                5.0 + 120.0 * f64::from(k as u32 / 2),
            );
            let mapping = GradMapping::Affine {
                a: Point64::new(x, y),
                b: Point64::new(x, y + h),
                c: Point64::new(x + w, y),
            };
            let paint = Paint::Image {
                image: id,
                mapping,
                repeat: Repeat::Simple,
                filter,
                contone: None,
                adjust: xarast_render::BitmapAdjust::default(),
            };
            b.image(SceneNodeId(k as u64), id, mapping, paint);
        }
        b.finish().expect("balanced");
    }
    let view = ViewParams::new(320, 240, Transform2D::IDENTITY, quality);
    (scene, res, view)
}

fn render_into(
    scene: &Scene,
    res: &xarast_render::Resolver,
    view: &ViewParams,
    cfg: CpuConfig,
    rect: Option<xarast_render::DeviceRect>,
    target: &mut Surface,
) {
    let dirty = rect.map_or(DirtyRect::NONE, DirtyRect::of);
    let dl = DisplayList::build(scene, view, &dirty);
    CpuBackend::new(cfg)
        .render(&dl, res, target)
        .expect("renders");
}

fn substituting() -> CpuConfig {
    CpuConfig {
        missing_levels: xarast_render::MissingLevels::Substitute,
        ..CpuConfig::deterministic()
    }
}

/// XARA-T-0281: a zoomed-out Draft frame of evicted photographs reads no
/// base back on the render thread, and the Final that follows — drawn
/// from substitutes, then repainted over the images once their bases are
/// back — is the unlimited render byte for byte.
#[test]
fn a_zoomed_out_draft_reads_no_base_back_and_the_final_converges() {
    let root = scratch("async");
    // The product's proxy sizes, nothing evictable kept resident.
    let budget = PixelBudget::new(BudgetConfig {
        limit_bytes: 0,
        spill_root: Some(root.clone()),
        ..BudgetConfig::unlimited()
    });
    let free = PixelBudget::new(BudgetConfig::unlimited());
    let data: Vec<Vec<u8>> = (0..4).map(|k| noisy(1200, 900, 10 + k)).collect();
    let squeezed: Vec<ImageRef> = data
        .iter()
        .map(|d| ImageRef::with_budget(1200, 900, d.clone(), &budget, None))
        .collect();
    let reference_images: Vec<ImageRef> = data
        .iter()
        .map(|d| ImageRef::with_budget(1200, 900, d.clone(), &free, None))
        .collect();
    // Evicted without a pyramid: the proxy (150 × 113) was built on the
    // way out, the base spilled.
    for image in &squeezed {
        assert!(!image.resident_levels()[0]);
        assert!(image.resident_levels()[image.proxy_level()]);
    }
    let reads = |b: &PixelBudget| {
        let s = b.stats();
        s.from_spill + s.from_source
    };

    // Draft at 1/8 scale (8 texels per pixel): level 3, the proxy. Even
    // the waiting policy reads nothing back.
    let before = reads(&budget);
    for cfg in [CpuConfig::deterministic(), substituting()] {
        let (scene, res, view) = photos(&squeezed, 0.125, RenderQuality::Draft);
        render_into(&scene, &res, &view, cfg, None, &mut Surface::new(320, 240));
    }
    assert_eq!(
        reads(&budget),
        before,
        "a zoomed-out Draft read a base back"
    );
    assert_eq!(budget.stats().substituted, 0);

    // Draft at 1/4 scale: level 2 is above the proxy and evicted. The
    // substituting render draws the proxy and reads nothing back.
    let tick = xarast_render::substitution_tick();
    let (scene, res, view) = photos(&squeezed, 0.25, RenderQuality::Draft);
    render_into(
        &scene,
        &res,
        &view,
        substituting(),
        None,
        &mut Surface::new(320, 240),
    );
    assert_eq!(reads(&budget), before, "the Draft waited for a base");
    assert!(budget.stats().substituted >= 4, "one per image and band");
    assert!(squeezed.iter().all(|i| i.substituted_since(tick)));

    // The Final at 1/4 scale, the way the render thread does it: drawn
    // from substitutes, the bases brought back off the render thread,
    // then the images' damage repainted over the frame.
    let (scene, res, view) = photos(&squeezed, 0.25, RenderQuality::Final);
    let mut frame = Surface::new(320, 240);
    let tick = xarast_render::substitution_tick();
    render_into(&scene, &res, &view, substituting(), None, &mut frame);
    assert_eq!(reads(&budget), before, "the Final waited for a base");
    let hit: Vec<bool> = res
        .images
        .iter()
        .map(|i| i.substituted_since(tick))
        .collect();
    assert!(hit.iter().all(|&h| h));
    let (ref_scene, ref_res, _) = photos(&reference_images, 0.25, RenderQuality::Final);
    let mut reference = Surface::new(320, 240);
    render_into(
        &ref_scene,
        &ref_res,
        &view,
        CpuConfig::deterministic(),
        None,
        &mut reference,
    );
    assert!(
        frame.data() != reference.data(),
        "a substitute is not the picture"
    );

    // A budget that keeps what it brings back: the tiny one would evict
    // the bases again before the repaint pins them.
    budget.set_limit(u64::MAX);
    for image in res.images.iter() {
        assert!(image.rematerialise());
    }
    assert_eq!(budget.stats().rematerialised, 4);
    let damage = xarast_render::image_damage(&scene, &view, |id| hit[id.index() as usize], 4)
        .expect("balanced");
    assert!(!damage.rects.is_empty());
    let tick = xarast_render::substitution_tick();
    for r in &damage.rects {
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                frame.set_pixel(x, y, [0, 0, 0, 0]);
            }
        }
        render_into(&scene, &res, &view, substituting(), Some(*r), &mut frame);
    }
    assert!(!squeezed.iter().any(|i| i.substituted_since(tick)));
    assert!(
        frame.data() == reference.data(),
        "the repaint did not converge"
    );

    // And the waiting policy, as export uses it, is exact at once.
    budget.set_limit(0);
    let mut direct = Surface::new(320, 240);
    render_into(
        &scene,
        &res,
        &view,
        CpuConfig::deterministic(),
        None,
        &mut direct,
    );
    assert!(direct.data() == reference.data());
    assert_eq!(budget.stats().lost, 0);
    drop((squeezed, res, scene));
    drop(budget);
    let _ = std::fs::remove_dir_all(&root);
}
