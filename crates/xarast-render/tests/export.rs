//! The export rasteriser's determinism contract (T11.2.2).
//!
//! An export is a function of (scene, area, pixel size, background) and
//! nothing else: not the run, not the strip budget, not the thread count.

use xarast_geom::{Mp, Rect};
use xarast_render::corpus::all_cases;
use xarast_render::export::{
    DEFAULT_STRIP_BUDGET, ExportJob, export_band_lines, render_export, render_export_strips,
};
use xarast_render::golden::digest;
use xarast_render::{RenderQuality, Surface};

const PX: u32 = 700;

fn job<'a>(case: &'a xarast_render::corpus::Case, budget: usize) -> ExportJob<'a> {
    let side = i32::try_from(case.view.viewport.width()).unwrap() * Mp::PER_PT;
    ExportJob {
        scene: &case.scene,
        resolver: &case.resolver,
        area: Rect::raw(0, 0, side, side),
        width: PX,
        height: PX + 13,
        quality: RenderQuality::Final,
        dpi: 96.0,
        background: [255, 255, 255, 0],
        strip_budget_bytes: budget,
    }
}

fn whole(j: &ExportJob<'_>) -> Surface {
    render_export(j, &|| false, &mut |_, _| {})
        .expect("renders")
        .0
}

#[test]
fn an_export_is_the_same_on_every_run_and_every_strip_budget() {
    let cases = all_cases();
    let mut inked = 0;
    for case in &cases {
        let first = whole(&job(case, DEFAULT_STRIP_BUDGET));
        inked += usize::from(first.data().chunks(4).any(|p| p[3] != 0));
        let a = digest(&first);
        let b = digest(&whole(&job(case, DEFAULT_STRIP_BUDGET)));
        assert_eq!(a, b, "{} changed between runs", case.name);
        // One band per strip: many strips, the same bytes.
        let c = digest(&whole(&job(case, 1)));
        assert_eq!(a, c, "{} depends on the strip height", case.name);
    }
    // The area really frames the content: an export of nothing would pass
    // every assertion above.
    assert!(
        inked * 10 > cases.len() * 9,
        "{inked} of {} drew",
        cases.len()
    );
}

#[test]
fn an_export_is_the_same_on_one_thread_and_on_many() {
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("pool");
    let many = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .expect("pool");
    for case in all_cases() {
        let j = job(&case, DEFAULT_STRIP_BUDGET);
        let a = one.install(|| digest(&whole(&j)));
        let b = many.install(|| digest(&whole(&j)));
        assert_eq!(a, b, "{} differs between 1 and 8 threads", case.name);
    }
}

#[test]
fn strips_arrive_in_order_and_cover_the_image() {
    let cases = all_cases();
    let j = job(&cases[0], 1);
    let mut next = 0u32;
    let mut calls = 0u32;
    let stats = render_export_strips(
        &j,
        &|| false,
        &mut |done, total| {
            assert!(done <= total);
        },
        &mut |y0, s: &Surface| -> Result<(), ()> {
            assert_eq!(y0, next);
            assert_eq!(s.width(), PX);
            next += s.height();
            calls += 1;
            Ok(())
        },
    )
    .expect("renders");
    assert_eq!(next, PX + 13);
    assert_eq!(stats.strips, calls);
    assert_eq!(stats.band_lines, export_band_lines(PX, PX + 13));
    assert!(calls > 1);
}

#[test]
fn cancelling_stops_the_export() {
    let cases = all_cases();
    let j = job(&cases[0], 1);
    let r = render_export(&j, &|| true, &mut |_, _| {});
    assert!(matches!(
        r,
        Err(xarast_render::export::ExportRenderError::Cancelled)
    ));
}

#[test]
fn a_bad_size_or_area_is_refused() {
    let cases = all_cases();
    let mut j = job(&cases[0], 1);
    j.width = 0;
    assert!(render_export(&j, &|| false, &mut |_, _| {}).is_err());
    let mut j = job(&cases[0], 1);
    j.area = Rect::EMPTY;
    assert!(render_export(&j, &|| false, &mut |_, _| {}).is_err());
}

#[test]
fn a_list_rasterised_through_its_own_commands_matches_the_export() {
    use xarast_render::export::ListRasteriser;
    use xarast_render::{DirtyRect, DisplayList};
    let mut r = ListRasteriser::new();
    for case in all_cases().iter().take(40) {
        let j = job(case, DEFAULT_STRIP_BUDGET);
        let expected = whole(&j);
        let view = j.view();
        let dl = DisplayList::build(&case.scene, &view, &DirtyRect::of(view.viewport));
        // The same commands through `with_commands`: the same bytes.
        let same = dl.with_commands(dl.commands().to_vec());
        let got = r
            .render(
                &same,
                &case.resolver,
                j.width,
                j.height,
                j.background,
                &|| false,
            )
            .expect("renders");
        assert_eq!(digest(&expected), digest(&got), "{}", case.name);
        // No commands: the background only.
        let none = dl.with_commands(Vec::new());
        let blank = r
            .render(
                &none,
                &case.resolver,
                j.width,
                j.height,
                j.background,
                &|| false,
            )
            .expect("renders");
        assert!(blank.data().chunks(4).all(|p| p == j.background));
    }
}
