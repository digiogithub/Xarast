//! The CPU backend is the oracle, so it has to be bit-reproducible.
//!
//! The W0 spike established what that costs: `vello_cpu` is byte-identical
//! over 100 runs at a fixed SIMD level, and differs in 3 of 786,432 channel
//! samples between the AVX2 and baseline paths. The deterministic
//! configuration therefore pins the level.

mod common;

use std::collections::BTreeSet;

use common::{render_case, render_case_with};
use xarast_render::CpuConfig;
use xarast_render::corpus::all_cases;
use xarast_render::golden::digest;

#[test]
fn the_whole_corpus_hashes_the_same_on_every_run() {
    let cases = all_cases();
    let baseline: Vec<String> = cases.iter().map(|c| digest(&render_case(c))).collect();
    for run in 1..20 {
        for (case, want) in cases.iter().zip(baseline.iter()) {
            let got = digest(&render_case(case));
            assert_eq!(&got, want, "run {run} changed {}", case.name);
        }
    }
}

#[test]
fn parallel_bands_produce_the_same_bytes_as_serial_ones() {
    // Determinism survives parallelism only if the merge order is fixed;
    // this is the test that says so.
    let serial = CpuConfig {
        threads: 1,
        ..CpuConfig::deterministic()
    };
    let parallel = CpuConfig {
        threads: 0,
        ..CpuConfig::deterministic()
    };
    for case in all_cases() {
        let a = digest(&render_case_with(&case, serial));
        let b = digest(&render_case_with(&case, parallel));
        assert_eq!(a, b, "{} differs between 1 and N threads", case.name);
    }
}

#[test]
fn the_band_height_does_not_change_the_pixels() {
    // Bands are an implementation detail of memory, not of appearance. If
    // this fails, something accumulates across a band boundary.
    for case in all_cases() {
        let mut digests = BTreeSet::new();
        for budget in [1 << 10, 1 << 14, 1 << 20, 1 << 26] {
            let cfg = CpuConfig {
                band_budget_bytes: budget,
                ..CpuConfig::deterministic()
            };
            digests.insert(digest(&render_case_with(&case, cfg)));
        }
        assert_eq!(digests.len(), 1, "{} depends on the band height", case.name);
    }
}

#[test]
fn the_interactive_configuration_is_not_claimed_to_be_deterministic() {
    use xarast_render::CpuBackend;
    assert!(
        CpuBackend::new(CpuConfig::deterministic())
            .capabilities()
            .deterministic
    );
    assert!(
        !CpuBackend::new(CpuConfig::interactive())
            .capabilities()
            .deterministic
    );
}
