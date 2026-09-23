---
id: XARA-T-0015
type: task
title: Workspace clippy gate fails on the 1.98 toolchain (pre-existing lints outside xarast-cli)
status: done
priority: high
parent: XARA-US-0001
author: mcp
labels: [ci, lint]
created: 2026-09-23T10:04:32Z
updated: 2026-09-23T10:36:14Z
started: 2026-09-23T10:09:51Z
closed: 2026-09-23T10:36:14Z
---

## Description
`rust-toolchain.toml` pins `stable`, which is now rustc/clippy 1.98.1. At base commit d43efd2, `cargo clippy --workspace --all-targets -- -D warnings` fails before it reaches most crates:

- `clippy::chunks_exact_to_as_chunks` (new lint): `xarast-render/src/golden.rs` (16 sites), `xarast-render/src/backend/cpu.rs` (3), `xarast-render/src/surface.rs` (1).
- `clippy::collapsible_match`: `xarast-doc/src/history.rs:335`.
- The float-fallback future-incompat lint (`1.0` → `1.0_f32` for `egui::Stroke::new`): `xarast-ui/src/theme.rs` and `xarast-ui/src/panels/colour.rs`.

XARA-T-0004 only touched `xarast-cli`. It was checked with `cargo clippy -p xarast-cli --all-targets --no-deps -- -D warnings`, which is clean.

## Acceptance Criteria
- `cargo clippy --workspace --all-targets -- -D warnings` is green.
- Either fix the sites (`as_chunks` needs Rust 1.88; the MSRV is 1.90) or pin the toolchain to a specific version so that a new stable does not break the gate.
