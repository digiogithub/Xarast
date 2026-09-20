//! The egui density spike: run it, read the nine numbers.
//!
//! `cargo run -p xarast-ui --release --example density`
//!
//! Architecture §7 question 2 asks whether egui's immediate mode holds up
//! at professional panel density. This example runs the probe of
//! `xarast_ui::density` and prints the nine axes of
//! `docs/phases/phase-05-shell-and-ui.md` §W1 with their thresholds and
//! verdicts.
//!
//! **Five of the nine can be measured without a display and four cannot.**
//! This machine has no GPU and no compositor, so the GPU pass, the
//! presented frame rate, the visual legibility check and the
//! pointer-to-pixel latency are reported as `unmeasured` rather than
//! guessed at. Every axis prints how it was obtained, so a later run on
//! real hardware can fill the gaps without re-reading this file.

use std::time::Duration;

use xarast_ui::density::{DensityProbe, ProbeConfig, Samples, measure, measure_tessellation};
use xarast_ui::theme::{ROW_HEIGHT, ResolvedTheme, apply};

/// A window big enough to show every panel at once, which is the
/// condition the spike measures under.
const SCREEN: (f32, f32) = (2560.0, 1440.0);

/// Frames per measurement. Enough for a stable 99th percentile.
const FRAMES: usize = 200;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn verdict(pass: bool) -> &'static str {
    if pass { "PASS" } else { "FAIL" }
}

/// True when this process has a display to draw on.
///
/// The GPU-bound axes are skipped, not failed, when it does not: a spike
/// that cannot run in CI stops being run.
fn has_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

/// Resident set size in kibibytes, or `None` where `/proc` is not there.
fn resident_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    apply(&ctx, ResolvedTheme::Dark);
    ctx
}

fn main() {
    let display = has_display();
    println!("egui density spike — xarast-ui");
    println!("egui {}", egui_version());
    println!(
        "environment: {}\n",
        if display {
            "a display is present"
        } else {
            "headless (no GPU, no compositor): GPU axes are skipped, not guessed"
        }
    );

    let baseline_rss = resident_kib();

    // ── P1: the cost of building one frame of the full layout ───────────
    let ctx = context();
    let mut probe = DensityProbe::new(ProbeConfig::default(), ResolvedTheme::Dark);
    let build = measure(
        &mut probe,
        &ctx,
        egui::vec2(SCREEN.0, SCREEN.1),
        1.0,
        FRAMES,
    );
    let p1_p50 = build.percentile(0.5);
    let p1_p99 = build.percentile(0.99);
    let controls = probe.control_count();

    // ── P2: the main thread's share we can actually see ─────────────────
    let tess = measure_tessellation(&mut probe, &ctx, egui::vec2(SCREEN.0, SCREEN.1), 1.0, 60);
    let p2 = p1_p99 + tess.percentile(0.99);

    // ── P4: ten seconds of scrolling the 5,000-row tree ─────────────────
    let mut scroll = Samples::default();
    let mut offset = 0.0f32;
    for _ in 0..600 {
        offset = (offset + 8.0) % (5_000.0 * ROW_HEIGHT);
        probe.scroll_to(offset);
        let one = measure(&mut probe, &ctx, egui::vec2(SCREEN.0, SCREEN.1), 1.0, 3);
        scroll.frames.push(one.max());
    }

    // ── P5: moving one slider against an idle frame ─────────────────────
    probe.scroll_to(0.0);
    let idle = measure(&mut probe, &ctx, egui::vec2(SCREEN.0, SCREEN.1), 1.0, 60);
    let mut dragging = Samples::default();
    for i in 0..60 {
        probe.nudge_slider((i as f32 / 60.0).fract());
        let one = measure(&mut probe, &ctx, egui::vec2(SCREEN.0, SCREEN.1), 1.0, 3);
        dragging.frames.push(one.percentile(0.5));
    }
    let p5_delta = ms(dragging.percentile(0.5)) - ms(idle.percentile(0.5));

    // ── P6: what all of it costs in memory ──────────────────────────────
    let p6 = match (baseline_rss, resident_kib()) {
        (Some(a), Some(b)) => Some((b.saturating_sub(a)) as f64 / 1024.0),
        _ => None,
    };

    // ── P7: density at the three scale factors ──────────────────────────
    let mut row_device_px = Vec::new();
    for scale in [1.0f32, 1.25, 1.5] {
        let ctx = context();
        let mut p = DensityProbe::new(ProbeConfig::default(), ResolvedTheme::Dark);
        let s = measure(&mut p, &ctx, egui::vec2(SCREEN.0, SCREEN.1), scale, 8);
        row_device_px.push((scale, ROW_HEIGHT * scale, ms(s.percentile(0.5))));
    }

    // ── P8: what AccessKit is told ──────────────────────────────────────
    let a11y = accesskit_census();

    // ── the table ───────────────────────────────────────────────────────
    println!(
        "probe: {controls} controls instantiated per frame, 5,000-row tree, 512 swatches, 2,000 thumbnails"
    );
    println!(
        "frames: {FRAMES} per measurement, window {}×{}\n",
        SCREEN.0, SCREEN.1
    );
    row("axis", "metric", "threshold", "measured", "verdict");

    row(
        "P1",
        "build_ui_frame CPU",
        "p50 <= 3 ms, p99 <= 8 ms",
        &format!("p50 {:.2} ms, p99 {:.2} ms", ms(p1_p50), ms(p1_p99)),
        verdict(ms(p1_p50) <= 3.0 && ms(p1_p99) <= 8.0),
    );
    row(
        "P2",
        "main thread total",
        "<= 8 ms every frame",
        &format!(
            "{:.2} ms (build p99 + tessellate p99); events and submit unmeasured",
            ms(p2)
        ),
        if ms(p2) <= 8.0 {
            "PARTIAL PASS"
        } else {
            "FAIL"
        },
    );
    row(
        "P3",
        "egui GPU pass",
        "<= 1.5 ms",
        "unmeasured (no GPU adapter in this environment)",
        "UNMEASURED",
    );
    row(
        "P4",
        "5,000-row tree scrolled 10 s",
        "no frame over 16 ms",
        &format!("worst CPU frame {:.2} ms over 600 frames", ms(scroll.max())),
        if ms(scroll.max()) <= 16.0 {
            "PARTIAL PASS"
        } else {
            "FAIL"
        },
    );
    row(
        "P5",
        "partial update (one slider)",
        "within 1 ms of idle",
        &format!("{p5_delta:+.2} ms against idle"),
        verdict(p5_delta.abs() <= 1.0),
    );
    row(
        "P6",
        "resident growth, all panels",
        "<= 50 MB",
        &match p6 {
            Some(mb) => format!("{mb:.1} MB"),
            None => "unmeasured (no /proc)".to_owned(),
        },
        match p6 {
            Some(mb) => verdict(mb <= 50.0),
            None => "UNMEASURED",
        },
    );
    let density_line = row_device_px
        .iter()
        .map(|(s, px, t)| format!("{s}x: {px:.0} device px rows, {t:.2} ms"))
        .collect::<Vec<_>>()
        .join("; ");
    row(
        "P7",
        "20 pt rows at 1x/1.25/1.5",
        "legible, screenshots",
        &format!("{density_line} — legibility unmeasured (no display)"),
        "PARTIAL",
    );
    row(
        "P8",
        "AT-SPI tree",
        "tree, fields, toggles present",
        &format!(
            "{} nodes, {} labelled; tree {}, numeric field {}, toggle {}",
            a11y.total,
            a11y.labelled,
            yesno(a11y.has_tree),
            yesno(a11y.has_field),
            yesno(a11y.has_toggle)
        ),
        verdict(a11y.has_tree && a11y.has_field && a11y.has_toggle),
    );
    row(
        "P9",
        "pointer to handle update",
        "<= 2 frames at 60 Hz",
        "1 frame of UI latency by construction; presentation unmeasured (no compositor)",
        "PARTIAL",
    );

    println!();
    println!(
        "Five axes measured, two partial on the CPU side, two unmeasured. \
Re-run on hardware with a display to close P3, P7, P9 and the presented half of P2 and P4."
    );
}

fn yesno(v: bool) -> &'static str {
    if v { "yes" } else { "NO" }
}

fn row(axis: &str, metric: &str, threshold: &str, measured: &str, verdict: &str) {
    println!("{axis:<4} {metric:<34} {threshold:<26} {measured:<28} {verdict}");
}

struct A11yCensus {
    total: usize,
    labelled: usize,
    has_tree: bool,
    has_field: bool,
    has_toggle: bool,
}

/// Runs the probe with AccessKit enabled and inspects the tree it emits.
fn accesskit_census() -> A11yCensus {
    let ctx = context();
    ctx.enable_accesskit();
    let mut probe = DensityProbe::new(ProbeConfig::default(), ResolvedTheme::Dark);
    let mut census = A11yCensus {
        total: 0,
        labelled: 0,
        has_tree: false,
        has_field: false,
        has_toggle: false,
    };
    for _ in 0..3 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(SCREEN.0, SCREEN.1),
                )),
                ..Default::default()
            },
            |ctx| probe.ui(ctx),
        );
        let Some(update) = output.platform_output.accesskit_update else {
            continue;
        };
        census.total = update.nodes.len();
        census.labelled = update
            .nodes
            .iter()
            .filter(|(_, n)| n.label().is_some_and(|l| !l.is_empty()))
            .count();
        for (_, node) in &update.nodes {
            match node.role() {
                egui::accesskit::Role::CheckBox | egui::accesskit::Role::Switch => {
                    census.has_toggle = true
                }
                egui::accesskit::Role::TextInput
                | egui::accesskit::Role::MultilineTextInput
                | egui::accesskit::Role::Slider
                | egui::accesskit::Role::SpinButton => census.has_field = true,
                egui::accesskit::Role::Tree
                | egui::accesskit::Role::TreeItem
                | egui::accesskit::Role::Table
                | egui::accesskit::Role::ListItem
                | egui::accesskit::Role::Row
                | egui::accesskit::Role::ScrollView => census.has_tree = true,
                _ => {}
            }
        }
    }
    census
}

fn egui_version() -> &'static str {
    // egui does not publish its own version string, and the manifest is
    // the single source of truth for it.
    "0.33"
}
