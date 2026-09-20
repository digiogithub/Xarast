//! Shared helpers for the integration tests.
//!
//! `tests/*.rs` are separate crates, so this is included with `mod common;`
//! rather than shared through the library.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use xarast_render::corpus::Case;
use xarast_render::{CpuBackend, CpuConfig, DirtyRect, DisplayList, Surface};

/// Renders one corpus case with the deterministic CPU configuration.
pub fn render_case(case: &Case) -> Surface {
    render_case_with(case, CpuConfig::deterministic())
}

/// Renders one corpus case with an explicit configuration.
pub fn render_case_with(case: &Case, cfg: CpuConfig) -> Surface {
    let dl = DisplayList::build(&case.scene, &case.view, &DirtyRect::NONE);
    let mut target = Surface::new(case.view.viewport.width(), case.view.viewport.height());
    let mut backend = CpuBackend::new(cfg);
    backend
        .render(&dl, &case.resolver, &mut target)
        .expect("the corpus fits in a surface");
    target
}

/// Where the committed golden images live.
pub fn golden_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Where failure artefacts are written.
pub fn artefact_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/golden-diffs")
        .components()
        .collect()
}

/// Whether the run is allowed to rewrite the committed goldens.
pub fn updating() -> bool {
    std::env::var("XARAST_UPDATE_GOLDEN").is_ok_and(|v| v != "0")
}
