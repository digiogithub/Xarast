# Phase 0 — Foundations & walking skeleton

> After this phase a person can download an artifact from any push, run it on a
> stock Wayland desktop, and see an empty Xarast window — and every later change
> is checked by CI that already enforces formatting, lints, licences and
> packaging.

## Goal

Build the scaffolding that every other phase depends on, and prove it end to
end by shipping something runnable. Concretely: a Cargo workspace containing all
thirteen crates of `docs/10-architecture.md §2` as compiling stubs; a CI
pipeline that runs `fmt`, `clippy`, `test`, `cargo deny` and `build`; and an
x86_64 AppImage, produced on *every* push, that opens a winit window with a wgpu
surface on Wayland and clears it to a known colour.

Alongside that, the three test harnesses that later phases plug into —
`cargo-fuzz`, `criterion`, and the golden-image comparator — plus the read-only
`.xar` corpus fixture.

Nothing in this phase implements product behaviour. If a task here needs a
decision about geometry, the document model or rendering, it is out of scope.

## Scope

### In scope

- **Workspace.** `Cargo.toml` finalised: `resolver = "3"`, `edition = "2024"`,
  `rust-version = "1.90"`, `license = "MIT OR Apache-2.0"`, the
  `[workspace.dependencies]` table, the four build profiles, workspace lints.
- **13 product crates** created as stubs under `crates/`, each with a
  `//!`-level module doc stating its responsibility and its position in the
  dependency graph, and each compiling with zero warnings.
- **1 non-shipping crate**, `xarast-testkit` (`publish = false`), used only as a
  `dev-dependency`. It holds the corpus fixture, the golden-image comparator and
  the deterministic RNG helpers. It is *not* a fourteenth product crate: it never
  appears in the dependency graph of any binary we ship, and CI asserts that.
- **Tooling config**: `rustfmt.toml`, lint policy, `deny.toml` (already present,
  extended), `rust-toolchain.toml`, `.cargo/config.toml`.
- **CI**: `.github/workflows/ci.yml` (check gate) and
  `.github/workflows/appimage.yml` (packaging, runs on every push).
- **Packaging**: AppDir layout, `AppRun`, desktop entry, icon set, AppStream
  metainfo, MIME registration for `.xarast` and `.xar`, zsync update
  information, size budget, distro smoke matrix.
- **Walking skeleton**: `xarast-shell` opens a window, acquires a wgpu surface,
  renders a clear pass, handles resize/close, exits cleanly. `xarast-cli` parses
  `--version`/`--help` and exits 0 without touching a display.
- **Harnesses**: `fuzz/` scaffolding with one trivial target; `benches/`
  scaffolding with one trivial benchmark per crate that will get budgets;
  golden-image harness with one self-test case; `.xar` corpus fixture with a
  checked-in manifest of hashes.

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner |
|---|---|
| Any geometry type, path or colour maths | Phase 1 |
| Node arena, attributes, undo | Phase 2 |
| Reading a single `.xar` record | Phase 3 |
| Drawing anything other than a solid clear colour | Phase 4 |
| Panels, menus, egui integration, file dialogs | Phase 5 |
| Tablet input, `octotablet`, pressure | Phase 5 (spike), Phase 7 (use) |
| aarch64 AppImage as a *gating* artifact (it is built, failures are non-blocking) | Phase 12 |
| Flatpak, Windows MSI, macOS `.app` | Phase 12 / Phase 14 |
| Code signing, notarisation, release notes automation | Phase 12 |
| Real performance budgets other than cold start and image size | Phases 1–5 |

## Prerequisites

None. This is the root of the dependency graph. The inputs are
`docs/10-architecture.md` (crate list and layering rule),
`docs/11-licensing-and-clean-room.md` (licence gate and corpus rule) and
`docs/research/05-technology-stack.md §11, §13` (packaging and testing).

The machine running the corpus tests must have the original fork checked out at
`/home/user/xara-xtreme`. CI does not, and must not.

## Workstreams

### W0.1 — Workspace and crate skeleton

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 0.1.1 | Finalise root `Cargo.toml`: members, workspace package metadata, `[workspace.dependencies]`, profiles `dev`/`release`/`dist`/`test` | — | M | — |
| 0.1.2 | Create the 13 product crate stubs with module docs and a `#![forbid(unsafe_code)]` where applicable | all | M | 0.1.1 |
| 0.1.3 | Create `xarast-testkit` (`publish = false`) | `xarast-testkit` | S | 0.1.1 |
| 0.1.4 | Wire the dependency edges exactly as `10-architecture.md §2` states; add the layering test | all | S | 0.1.2 |
| 0.1.5 | `rustfmt.toml`, lint tables, `.cargo/config.toml` | — | S | 0.1.1 |
| 0.1.6 | `rust-toolchain.toml` pinned to `stable` with `rustfmt`, `clippy`, plus a documented nightly used only by `cargo-fuzz` | — | S | — |

The **layering rule is mechanically enforced**, not merely documented. A test in
`xarast-testkit` runs `cargo metadata --format-version 1 --no-deps` plus the
resolved graph and asserts:

1. No crate at or below `xarast-app` has `egui`, `winit`, `wgpu` or `accesskit`
   in its transitive dependencies. This is the rule that makes headless CI
   rendering possible (`10-architecture.md §2`).
2. `xarast-testkit` is in no shipped binary's dependency closure.
3. There is no dependency cycle and no edge that the architecture table does not
   list. New edges are a deliberate edit to both the test's table and the
   architecture document.

Crate stubs and their binaries:

| Crate | Produces | Notes |
|---|---|---|
| `xarast-geom` … `xarast-io` | rlib | pure libraries |
| `xarast-app` | rlib | UI-toolkit free |
| `xarast-ui` | rlib | `egui` allowed from here up |
| `xarast-shell` | rlib **+ bin `xarast`** | the GUI entry point |
| `xarast-cli` | rlib **+ bin `xarast-cli`** | `xar-dump` is added here in Phase 3 |

`panic` strategy: the root `Cargo.toml` currently sets `panic = "abort"` for
release. **Change it to `panic = "unwind"`.** A vector editor must be able to
catch a panic in a worker (a boolean op, an image decode, a rasteriser tile) and
still offer the user a chance to save. `research/05` reaches the same conclusion
for the same reason. Record this in `docs/memory/packaging.md`.

### W0.2 — Lints, formatting, licence gate

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 0.2.1 | `rustfmt.toml` restricted to stable-only options | — | S | 0.1.5 |
| 0.2.2 | Workspace lint tables (`[workspace.lints.rust]`, `[workspace.lints.clippy]`) and per-crate overrides | all | S | 0.1.2 |
| 0.2.3 | Extend `deny.toml`: advisories, sources, bans, the MPL-2.0 exception mechanism | — | S | — |
| 0.2.4 | `cargo about` template + `THIRD-PARTY-LICENSES.html` generation | — | S | 0.2.3 |

`rustfmt.toml` uses **only stable options**, because CI formats with stable
rustfmt and a nightly-only key makes `cargo fmt --check` fail with a confusing
error:

```toml
edition = "2024"
max_width = 100
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
```

`imports_granularity` and `group_imports` are nightly-only and therefore **not**
used. Import order is left to the author.

Lint policy beyond what the root `Cargo.toml` already sets:

```toml
[workspace.lints.rust]
unsafe_code                   = "warn"     # "forbid" per crate where it applies
missing_debug_implementations = "warn"
missing_docs                  = "warn"
unreachable_pub               = "warn"
rust_2024_compatibility       = "warn"

[workspace.lints.clippy]
all                        = { level = "warn", priority = -1 }
undocumented_unsafe_blocks = "warn"
todo                       = "warn"
dbg_macro                  = "warn"
cast_possible_truncation   = "warn"   # a numeric-format parser lives or dies on this
cast_sign_loss             = "warn"
cast_precision_loss        = "warn"
```

`#![forbid(unsafe_code)]` goes in every crate except `xarast-shell`,
`xarast-render` and `xarast-image`, which have justified FFI or `bytemuck`
boundaries. CI greps for `unsafe` outside those three and fails on a hit.

`deny.toml` additions: `[advisories] unmaintained = "warn"`, `ignore = []` (each
future entry needs a dated comment and a reason); `[bans] deny` gains
`openssl-sys` (use `rustls` if networking ever appears). The `exceptions` list
stays empty until a real MPL-2.0 dependency lands; adding one is a reviewable
commit that must name the crate and why it is acceptable unmodified.

### W0.3 — The walking skeleton

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 0.3.1 | `ShellConfig` + `run()`: winit event loop, Wayland preferred, X11 fallback | `xarast-shell` | M | 0.1.2 |
| 0.3.2 | wgpu instance/adapter/device/surface creation with a documented backend order and a software fallback | `xarast-shell` | M | 0.3.1 |
| 0.3.3 | Clear-colour render pass, resize, scale-factor change, close | `xarast-shell` | S | 0.3.2 |
| 0.3.4 | `--version`, `--help`, `--selftest`, `--selftest-window` command-line surface | `xarast-shell`, `xarast-cli` | S | 0.3.1 |
| 0.3.5 | `tracing` + `tracing-subscriber` init, `XARAST_LOG` env filter, panic hook that logs and re-raises | `xarast-shell` | S | 0.3.1 |
| 0.3.6 | Cold-start instrumentation: log the monotonic time from `main` entry to first presented frame | `xarast-shell` | S | 0.3.3 |

The tricky parts:

**Wayland `app_id` must equal the desktop-entry basename.** winit's
`with_application_id("xarast")` on Wayland (and `with_name` on X11) has to match
`xarast.desktop`, or GNOME and KDE show a generic icon and group windows wrong.
This is the single most commonly missed step in Linux packaging and it costs
nothing to get right now. Assert it in code: the `app_id` constant and the
desktop-file name come from the same `const APP_ID: &str = "xarast";` and a test
reads `packaging/linux/xarast.desktop` and compares.

**Backend selection.** Request Vulkan first, then GL, and accept a software
adapter (`lavapipe`/`llvmpipe`) rather than failing. `--selftest-window` prints
the adapter name, backend and device type, which is exactly the diagnostic we
need from a user's bug report and exactly what the CI smoke job asserts. Do
**not** pin `winit` to the `0.31` beta in this phase: the beta is only needed for
`TabletToolData`, which Phase 5 evaluates (`10-architecture.md §7` question 3).
Phase 0 takes the newest stable `winit` that works, and records the version in
`docs/memory/packaging.md` so Phase 5 knows what it is moving from.

**Headless verification.** `--selftest-window` must be runnable with no
compositor visible to a human. The plan is `weston --backend=headless` inside the
CI container with `WAYLAND_DISPLAY=wayland-ci`, `WGPU_BACKEND=vulkan` and the
lavapipe ICD installed. If Weston's headless backend proves unreliable in the
`ubuntu:22.04` container, the fallback is `Xvfb` plus the X11 path, which tests
less but still tests surface creation. Which of the two CI ends up using is
**to be determined in this phase**, decided by running both in a throwaway
branch and keeping whichever is green three runs in a row.

### W0.4 — AppImage packaging

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 0.4.1 | `packaging/linux/xarast.desktop` | — | S | — |
| 0.4.2 | `packaging/linux/xarast.xml` — MIME definitions for `.xarast` and `.xar` | — | S | — |
| 0.4.3 | `packaging/linux/es.digio.Xarast.metainfo.xml` — AppStream | — | S | 0.4.1 |
| 0.4.4 | Icon set: SVG master plus rendered PNGs at 16/22/24/32/48/64/128/256/512 | `assets/icons` | M | — |
| 0.4.5 | `packaging/linux/AppRun` and the AppDir assembly script | — | M | 0.4.1–0.4.4 |
| 0.4.6 | `linuxdeploy` invocation with the exclusion list and `UPDATE_INFORMATION` | — | M | 0.4.5 |
| 0.4.7 | Size measurement and the `≤ 35 MB` Phase 0 gate | — | S | 0.4.6 |
| 0.4.8 | Distro smoke matrix | — | M | 0.4.6 |

**Tool choice: `linuxdeploy` + `linuxdeploy-plugin-appimage`.** The three
candidates:

- **`cargo-appimage`** — rejected. It is a thin wrapper around `linuxdeploy`
  with far less control, it is lightly used (≈1.5 K downloads/90 d per
  `research/05 §11.1`), and it is GPL-3.0. Being a build tool it does not infect
  our licence, but taking a dependency on an under-maintained wrapper to save
  twenty lines of shell is a bad trade for the component that decides whether
  users can run the product at all.
- **`appimage-builder`** — rejected. Its recipe model resolves dependencies
  through the distro package manager and bundles them aggressively, including
  libraries that our exclusion rule says must come from the user's system (Mesa,
  libdrm, fontconfig). Fighting a tool's defaults on the exact axis that matters
  most is the wrong shape. It is also a Python tool with a heavier CI surface
  than a single static binary. *Its current maintenance status is not recorded in
  `research/05` and is **to be determined in this phase** — check the upstream
  release feed once; if it turns out to be actively maintained the rejection
  still stands on the bundling-policy argument alone.*
- **`linuxdeploy` + `linuxdeploy-plugin-appimage`** — chosen. MIT. We build the
  AppDir explicitly, so we know exactly what ships. `--exclude-library` gives us
  the "hardware and compositor libraries come from the system" rule directly.
  The `appimage` plugin wraps `appimagetool` and supports `UPDATE_INFORMATION`,
  which is how zsync metadata gets embedded. Both tools now default to the
  **static AppImage runtime**, which is what makes one file start on hosts that
  have libfuse2 *and* hosts that only have libfuse3.

**glibc baseline: 2.35, from a `ubuntu:22.04` container.** Rationale from
`research/05 §11.2`: 2.35 covers Debian 12, Ubuntu 22.04+, Fedora 36+, RHEL 9,
openSUSE 15.5+ and SteamOS 3.5+. Building *inside an explicit container* rather
than on the runner image is what makes the baseline reproducible when GitHub
rotates its images. Do **not** link musl: it breaks `dlopen` of the system GL and
Vulkan drivers, which is the one thing an AppImage must not break.

`RUSTFLAGS="-C target-cpu=x86-64-v2"` on x86_64 only (SSE4.2/POPCNT, safe since
2009); the aarch64 job uses the default target CPU.

**AppDir layout** (from `research/05 §11.4`):

```
Xarast.AppDir/
├── AppRun
├── xarast.desktop
├── xarast.png                (256x256)
├── .DirIcon -> xarast.png
└── usr/
    ├── bin/xarast
    ├── lib/                  (only non-excluded deps)
    └── share/
        ├── applications/xarast.desktop
        ├── icons/hicolor/{16,22,24,32,48,64,128,256,512}x*/apps/xarast.png
        ├── icons/hicolor/scalable/apps/xarast.svg
        ├── mime/packages/xarast.xml
        ├── metainfo/es.digio.Xarast.metainfo.xml
        └── doc/xarast/THIRD-PARTY-LICENSES.html
```

`AppRun` is a short shell script: it sets `XDG_DATA_DIRS` to prepend
`$APPDIR/usr/share` (so the bundled MIME and icon data are visible to the app
itself), sets `APPDIR`-relative paths for our own assets, and `exec`s
`$APPDIR/usr/bin/xarast "$@"`. It must **not** set `LD_LIBRARY_PATH` to a
bundled Mesa; `linuxdeploy`'s own `AppRun` behaviour plus the exclusion list
handles the library path correctly.

**Exclusion list** passed as repeated `--exclude-library`: `libGL*`, `libGLX*`,
`libEGL*`, `libgbm*`, `libdrm*`, `libvulkan*`, `libwayland-*`, `libX11*`,
`libxcb*`, `libxkbcommon*`, `libfontconfig*`, `libfreetype*`, plus the C runtime
(`libc`, `libm`, `libstdc++`, `libgcc_s`, `libpthread`, `libdl`). Bundling
fontconfig in particular is a classic own-goal: the user stops seeing their own
fonts.

**Desktop entry.** All user-visible strings in English (the language rule in
`CLAUDE.md` is absolute, including `Comment=`), with translations added in
Phase 12 as `Comment[xx]=` keys:

```ini
[Desktop Entry]
Type=Application
Name=Xarast
GenericName=Vector Graphics Editor
Comment=Vector illustration and photo editing
Exec=xarast %F
Icon=xarast
Categories=Graphics;VectorGraphics;RasterGraphics;2DGraphics;
MimeType=application/vnd.xarast+zip;application/x-xara;image/svg+xml;image/png;image/jpeg;
StartupNotify=true
StartupWMClass=xarast
```

**MIME registration.** `research/05 §11.4` proposed `application/x-xarast`;
`research/06 §3.2.1` specifies the container's `mimetype` entry as
**`application/vnd.xarast+zip`**. The format specification wins — the string in
the desktop entry, the MIME package and the ZIP `mimetype` member must be the
same byte sequence, or magic detection silently fails.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/vnd.xarast+zip">
    <comment>Xarast document</comment>
    <glob pattern="*.xarast"/>
    <magic priority="60">
      <match type="string" value="PK\003\004" offset="0">
        <match type="string" value="mimetype" offset="30">
          <match type="string" value="application/vnd.xarast+zip" offset="38"/>
        </match>
      </match>
    </magic>
    <icon name="application-vnd.xarast+zip"/>
  </mime-type>
  <mime-type type="application/x-xara">
    <comment>Xara drawing</comment>
    <glob pattern="*.xar"/>
    <glob pattern="*.web"/>
    <magic priority="60">
      <match type="string" value="XARA\xa3\xa3\r\n" offset="0"/>
    </magic>
  </mime-type>
</mime-info>
```

The `.xar` magic is the full eight bytes `58 41 52 41 A3 A3 0D 0A`
(`research/01 §1.2`), not just `"XARA"`: the `A3 A3 0D 0A` tail is the format's
own anti-ASCII-transfer guard and using it makes false positives impossible.

Note that an AppImage's MIME data is only seen by the desktop after the user
runs something like `appimaged` or installs the file by hand. Xarast therefore
also registers its types at first run into `~/.local/share/mime/packages/` and
calls `update-mime-database` — **deferred to Phase 12**, not done here; Phase 0
only has to ship correct data inside the AppDir.

**Icon.** One SVG master at `assets/icons/xarast.svg`, rasterised to the nine
PNG sizes by the build script. The icon must be our own artwork: nothing may be
traced from `xaralx.png`/`xaralx.xpm` in the original tree
(`docs/11-licensing-and-clean-room.md §3.2`). A placeholder mark is acceptable
for Phase 0 as long as it is original; the final identity is Phase 12.

**zsync.** `UPDATE_INFORMATION` is exported before invoking the `appimage`
plugin:

```
gh-releases-zsync|<owner>|<repo>|latest|Xarast-*-x86_64.AppImage.zsync
```

`research/05 §11.5` writes `digio-es|Xarast`, while the workspace
`Cargo.toml` says `github.com/digiogithub/Xarast`. These disagree. Derive the
string from `${{ github.repository }}` in the workflow so it cannot drift, and
record the resolved value in `docs/memory/packaging.md`. The plugin emits a
`.zsync` beside the `.AppImage`; **both** must be uploaded to the same release or
delta updates silently fall back to full downloads. Typical delta for a 60 MB
image is 3–8 MB.

**Size budget.** The roadmap sets `≤ 80 MB` for the shipping product. An empty
window has no fonts, no icons beyond ours, no image codecs and no UI, so Phase 0
gates at **`≤ 35 MB`** for the x86_64 AppImage. The gate is a CI step that reads
the file size and fails over the limit, and it prints the top ten largest
entries of the AppDir so a regression is diagnosable at a glance.

**Testing an AppImage where there is no FUSE.** GitHub runners have no
`/dev/fuse`. Every AppImage invocation in CI — both the `linuxdeploy` tools
themselves and our own output — must pass `--appimage-extract-and-run`, which
unpacks to a temporary directory and executes from there instead of mounting.
Set `APPIMAGE_EXTRACT_AND_RUN=1` in the job environment as a belt-and-braces
measure, since some tools read the variable rather than the flag. Extraction
costs about a second and is otherwise behaviourally identical, with one
exception worth knowing: `$APPIMAGE` is unset in that mode, so any code that
reads it (the future self-update check) must tolerate its absence.

**Distro smoke matrix.** A separate CI job downloads the artifact and runs it
inside `debian:12`, `fedora:41` and `archlinux:latest` containers with
`--appimage-extract-and-run --version`, then `--selftest-window` under the
headless compositor. This is what catches a forgotten runtime dependency, and it
is also what proves the static runtime claim (Debian 12 has libfuse2, Arch has
only libfuse3).

### W0.5 — CI

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 0.5.1 | `ci.yml`: fmt, clippy, test, deny, doc, bench-compile | — | M | W0.2 |
| 0.5.2 | `appimage.yml`: build in container, package, smoke, upload on every push; release on tags | — | L | W0.4 |
| 0.5.3 | `nightly.yml`: fuzz targets 30 min each, corpus cache | — | M | W0.6 |
| 0.5.4 | Caching (`Swatinem/rust-cache`), concurrency groups, job timeouts | — | S | 0.5.1 |
| 0.5.5 | Branch protection notes in `docs/memory/packaging.md` | — | S | 0.5.1 |

The check gate runs, in one job, on `ubuntu-latest`:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc
cargo deny check licenses advisories bans sources
cargo doc --workspace --no-deps            # RUSTDOCFLAGS="-D warnings"
cargo bench --workspace --no-run           # benchmarks must compile
```

`cargo nextest` does not run doctests, hence the separate `cargo test --doc`
line; forgetting it is how doctests rot.

The AppImage workflow triggers on `push` (any branch), `pull_request` and
`workflow_dispatch`, and additionally creates a GitHub Release on `v*` tags. The
x86_64 job is **required**; the `ubuntu-22.04-arm` job is
`continue-on-error: true` in this phase so that an ARM toolchain hiccup cannot
block the whole project on day one.

### W0.6 — Test and benchmark harnesses

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 0.6.1 | `fuzz/` scaffolding (own workspace, one trivial target) + nightly job | `fuzz` | M | 0.1.1 |
| 0.6.2 | `criterion` scaffolding: one benchmark per crate that will carry a budget, plus the budget-assertion helper | `xarast-testkit` | M | 0.1.3 |
| 0.6.3 | Golden-image harness skeleton + one self-test case | `xarast-testkit` | M | 0.1.3 |
| 0.6.4 | `.xar` corpus fixture and `corpus.lock` | `xarast-testkit` | M | 0.1.3 |
| 0.6.5 | Deterministic-RNG and temp-dir helpers | `xarast-testkit` | S | 0.1.3 |

**`cargo-fuzz`.** `fuzz/` is its own workspace (it must be, because libFuzzer
targets need nightly and their own profile), excluded from the root workspace
members. Phase 0 ships one target, `fuzz_smoke`, whose body is
`let _ = std::hint::black_box(data.len());`. Its purpose is to prove the whole
chain works — nightly toolchain, `cargo fuzz build`, corpus directory layout,
CI cache key, artifact upload on crash — *before* Phase 3 needs it in anger. The
nightly workflow runs every target for 30 minutes with
`-max_total_time=1800 -rss_limit_mb=2048` and caches `fuzz/corpus/` between runs.

**`criterion`.** One `benches/` file per crate that will eventually carry a
budget (`xarast-geom`, `xarast-doc`, `xarast-xar`, `xarast-render`). Phase 0's
benchmarks measure something trivial and exist to establish the shape. The
important piece is the helper that turns a measurement into a **gate**:

```rust
xarast_testkit::budget::assert_within("open_5mb_xar", measured, Budget::ms(500));
```

Criterion's own regression detection compares against the previous run on the
same machine, which is meaningless on ephemeral CI runners. Our budgets are
therefore **absolute numbers with generous headroom**, checked by the helper
above and recorded in `docs/memory/perf.md`, not by criterion's statistical
comparison. Criterion still earns its place for local work: `cargo bench` with
HTML reports is how a developer finds the regression the absolute gate caught.

**Golden-image harness.** The skeleton, with no renderer behind it yet:

- Cases live at `tests/golden/<area>/<case>.png` with a sibling
  `<case>.toml` describing the input and the comparison mode.
- `Comparison::Exact` is the default and the only mode allowed in the required
  CI gate. `Comparison::Perceptual { max_rms, max_channel_delta }` exists for
  the GPU-versus-CPU parity job that Phase 4 adds.
- `XARAST_GOLDEN_UPDATE=1 cargo test` rewrites the expected images; the harness
  refuses to do so when `CI` is set.
- On failure the harness writes `target/golden-failures/<case>.{actual,diff}.png`
  and prints the path; the CI job uploads that directory as an artifact.
- `10-architecture.md §7` question 5 asks whether exact or perceptual is the
  right gate. This phase does not answer it — it builds a harness that supports
  both so Phase 3/4 can answer it with data.

Phase 0's self-test case renders an 8×8 RGBA buffer filled with a constant and
compares it. If that case ever fails, the harness is broken, not the renderer.

**The `.xar` corpus fixture.** The 59 files are
`testfiles/*.xar` (18), `Designs/*.xar` (19), `Templates/*.xar` (8) and
`TextDesigns/*.xar` (14) under the original fork — 12 MB in total. Note that the
fork contains 73 `.xar` files; the other 14 (`wxOil/xrc/**`, `filters/SVGFilter/samples/**`)
are duplicates of the templates and SVG-filter samples and are **not** part of
the 59-file corpus that `research/01 §12.2` measured. The fixture must select
exactly the four directories above, or the counts in Phase 3's acceptance
criteria stop meaning anything.

Clean-room handling (`docs/11-licensing-and-clean-room.md §3.2`): these files are
**used locally and never redistributed**. Therefore:

- The fixture locates the corpus through `XARAST_XAR_CORPUS`, defaulting to
  `/home/user/xara-xtreme`. Nothing is copied into this repository.
- What *is* checked in is `tests/corpus/corpus.lock`: for each of the 59 files,
  its relative path, byte size and SHA-256. Hashes are facts, not expression, so
  this is clean-room safe, and it lets us detect a corpus that has drifted
  without shipping a single byte of Xara's artwork.
- `XarCorpus::discover() -> Option<XarCorpus>` returns `None` when the corpus is
  absent. Corpus tests then skip with a printed one-line notice, so CI (which has
  no corpus) is green while a developer's machine runs the real thing.
- `XARAST_CORPUS_REQUIRED=1` flips skipping into failure. The phase gate for
  Phase 3 is run with that variable set.
- A `.gitignore` rule and a CI check reject any `*.xar` file added under the
  Xarast repository, so the rule cannot be broken by accident.

## Public API introduced

```rust
// ─── xarast-shell ────────────────────────────────────────────────────────────

/// The Wayland/X11 application identifier. Must equal the basename of
/// `packaging/linux/xarast.desktop`; a test enforces that.
pub const APP_ID: &str = "xarast";

#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// Initial logical window size, in logical pixels.
    pub size: (u32, u32),
    /// Window title.
    pub title: String,
    /// Preferred wgpu backends, tried in order.
    pub backends: BackendPreference,
    /// Exit after presenting `Some(n)` frames. `None` runs until closed.
    pub exit_after_frames: Option<u32>,
}

impl Default for ShellConfig { /* 1280x800, "Xarast", Vulkan→GL, None */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPreference { VulkanThenGl, GlOnly, Auto }

/// Information about the adapter the shell actually got. Printed by
/// `--selftest-window` and included in bug reports.
#[derive(Debug, Clone)]
pub struct AdapterReport {
    pub name: String,
    pub backend: &'static str,
    pub device_type: &'static str,
    pub is_software: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("no compatible GPU adapter (tried {tried})")] NoAdapter { tried: String },
    #[error("failed to create a window: {0}")]            Window(String),
    #[error("surface configuration failed: {0}")]         Surface(String),
    #[error("event loop error: {0}")]                     EventLoop(String),
}

/// Runs the application to completion. Blocks the calling thread.
pub fn run(config: ShellConfig) -> Result<(), ShellError>;

/// Creates a window and a surface, presents one frame, tears everything down,
/// and reports what it got. This is the CI liveness check; it never needs a
/// human-visible display, only a compositor socket.
pub fn selftest_window(config: &ShellConfig) -> Result<AdapterReport, ShellError>;

/// Milliseconds from process start to the first presented frame, measured by
/// the shell itself. `None` before the first frame.
pub fn cold_start_ms() -> Option<f64>;

// ─── xarast-testkit (dev-dependency only) ────────────────────────────────────

pub mod corpus {
    use std::path::{Path, PathBuf};

    /// One corpus entry, as recorded in `tests/corpus/corpus.lock`.
    #[derive(Debug, Clone)]
    pub struct CorpusFile {
        /// Path relative to the corpus root, e.g. `testfiles/OneLine.xar`.
        pub rel: PathBuf,
        pub size: u64,
        pub sha256: [u8; 32],
    }

    impl CorpusFile {
        pub fn path(&self, root: &Path) -> PathBuf;
        pub fn read(&self, root: &Path) -> std::io::Result<Vec<u8>>;
        /// Short name used in test output and in `xar-dump` reports.
        pub fn label(&self) -> &str;
    }

    #[derive(Debug, Clone)]
    pub struct XarCorpus { root: PathBuf, files: Vec<CorpusFile> }

    impl XarCorpus {
        /// Reads `XARAST_XAR_CORPUS` (default `/home/user/xara-xtreme`), verifies
        /// every entry of `corpus.lock` exists with the recorded size and hash,
        /// and returns the corpus. `None` when the root is absent.
        ///
        /// Panics when `XARAST_CORPUS_REQUIRED=1` and the corpus is missing or
        /// has drifted from the lock file.
        pub fn discover() -> Option<Self>;
        pub fn root(&self) -> &Path;
        pub fn files(&self) -> &[CorpusFile];
        /// Exactly 59 for the recorded corpus.
        pub fn len(&self) -> usize;
        /// Regenerates `corpus.lock` from the corpus on disk. Never called by tests.
        pub fn write_lock(&self, to: &Path) -> std::io::Result<()>;
    }

    /// Skips the calling test with a printed notice when the corpus is absent.
    #[macro_export]
    macro_rules! corpus_or_skip { () => { /* ... */ } }
}

pub mod golden {
    use std::path::Path;

    #[derive(Debug, Clone, Copy)]
    pub enum Comparison {
        /// Byte-for-byte. The only mode permitted in the required CI gate.
        Exact,
        /// For the GPU-vs-CPU parity job only.
        Perceptual { max_rms: f64, max_channel_delta: u8 },
    }

    #[derive(Debug)]
    pub struct GoldenResult {
        pub differing_pixels: u64,
        pub rms: f64,
        pub max_channel_delta: u8,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum GoldenError {
        #[error("golden image `{0}` does not exist; run with XARAST_GOLDEN_UPDATE=1")]
        Missing(String),
        #[error("size mismatch: expected {expected:?}, got {actual:?}")]
        SizeMismatch { expected: (u32, u32), actual: (u32, u32) },
        #[error("{case}: {result:?} (artifacts in {dir})")]
        Mismatch { case: String, result: GoldenResult, dir: String },
    }

    /// Compares `actual` (RGBA8, row-major, no padding) against
    /// `tests/golden/<case>.png`. Writes `.actual.png` and `.diff.png` under
    /// `target/golden-failures/` on mismatch. Honours `XARAST_GOLDEN_UPDATE`,
    /// which it refuses to honour when `CI` is set.
    pub fn assert_golden(
        case: &str,
        size: (u32, u32),
        actual: &[u8],
        mode: Comparison,
    ) -> Result<(), GoldenError>;

    pub fn golden_dir() -> &'static Path;
}

pub mod budget {
    /// An absolute performance budget. Criterion's run-to-run comparison is
    /// meaningless on ephemeral runners, so budgets are absolute numbers.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Budget { pub nanos: u128, pub label: &'static str }

    impl Budget {
        pub const fn ms(v: u64) -> Budget;
        pub const fn us(v: u64) -> Budget;
        pub const fn bytes(v: u64) -> Budget;   // for memory budgets
    }

    /// Fails the test when `measured` exceeds `budget`, and always appends a
    /// line to `target/perf-report.jsonl` so CI can archive the numbers.
    pub fn assert_within(name: &str, measured: std::time::Duration, budget: Budget);
}
```

## Acceptance criteria

1. `cargo build --workspace --all-targets` succeeds from a clean checkout with
   `--locked`, and `cargo tree -e no-dev --workspace` lists exactly 13 product
   crates plus their external dependencies.
2. `cargo fmt --all -- --check` exits 0.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   exits 0.
4. `cargo nextest run --workspace` and `cargo test --workspace --doc` both
   exit 0.
5. `cargo deny check licenses advisories bans sources` exits 0.
6. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` exits 0.
7. The layering test passes: no crate at or below `xarast-app` has `winit`,
   `wgpu`, `egui` or `accesskit` in its resolved dependency graph, and
   `xarast-testkit` is absent from the dependency closure of the `xarast` and
   `xarast-cli` binaries.
8. A push to any branch produces a downloadable `Xarast-*-x86_64.AppImage`
   artifact, and the workflow run is green.
9. `./Xarast-*-x86_64.AppImage --appimage-extract-and-run --version` prints the
   version and exits 0 in each of `debian:12`, `fedora:41` and
   `archlinux:latest`.
10. `./Xarast-*-x86_64.AppImage --appimage-extract-and-run --selftest-window`
    exits 0 under the headless compositor and prints a non-empty adapter name.
11. `desktop-file-validate` and `appstreamcli validate --no-net` both exit 0 on
    the packaged files.
12. `xdg-mime query filetype` (run against the AppDir's MIME package in a
    container with `update-mime-database` applied) reports
    `application/vnd.xarast+zip` for a `.xarast` sample and
    `application/x-xara` for a corpus `.xar` file.
13. `stat -c %s Xarast-*-x86_64.AppImage` is `≤ 36 700 160` bytes (35 MiB), and
    the CI log lists the ten largest AppDir entries.
14. A `Xarast-*-x86_64.AppImage.zsync` file is produced alongside the AppImage,
    and `readelf -p .upd_info Xarast-*.AppImage` shows a `gh-releases-zsync|…`
    string whose owner/repo match `${{ github.repository }}`.
15. `cargo +nightly fuzz build` succeeds and `cargo +nightly fuzz run fuzz_smoke
    -- -runs=10000` exits 0.
16. `cargo bench --workspace --no-run` succeeds, and running the Phase 0
    benchmarks writes `target/perf-report.jsonl`.
17. `cargo nextest run -p xarast-testkit` passes the golden-image self-test, and
    deliberately corrupting one pixel makes it fail with an artifact written to
    `target/golden-failures/`.
18. With `XARAST_XAR_CORPUS` pointing at the fork and `XARAST_CORPUS_REQUIRED=1`,
    `XarCorpus::discover()` returns 59 files, all matching `corpus.lock` in size
    and SHA-256; with the variable unset and the path absent, corpus tests skip
    and the suite is still green.
19. `git ls-files '*.xar'` inside the Xarast repository returns nothing, and the
    CI check that enforces this is present and passing.
20. Cold start, measured as the median of five runs of
    `xarast --exit-after-frames 1` on the reference machine, is `≤ 400 ms`, and
    the number is recorded in `docs/memory/perf.md`.

## Performance budgets

| Budget | Target | How measured | Gate |
|---|---|---|---|
| Cold start to first presented frame | ≤ 400 ms | median of 5 × `xarast --exit-after-frames 1`, warm page cache | Fails the phase gate |
| AppImage size, x86_64, empty window | ≤ 35 MiB | `stat -c %s` in CI | Fails the build |
| AppImage size, shipping product | ≤ 80 MB | same, from Phase 12 | Recorded now, gated later |
| Clean `cargo build --workspace` (release) | ≤ 8 min on a 4-core runner | CI job duration | Warning only |
| Check gate wall clock (warm cache) | ≤ 6 min | CI job duration | Warning only |
| AppImage workflow wall clock | ≤ 20 min per arch | CI job duration | Warning only |
| Resident memory, empty window | ≤ 200 MB RSS | `/usr/bin/time -v` in the smoke job | Recorded, gated from Phase 5 |

The two hard numbers (cold start, image size) come from
`docs/phases/00-roadmap.md`. Everything else is recorded so that later phases can
see the trend rather than discovering a slow build in month six.

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | Headless Weston is flaky in the `ubuntu:22.04` container, so the window selftest cannot run in CI | Medium | High — without it the AppImage is only proven to *start*, not to *draw* | Evaluate Weston headless and Xvfb+X11 side by side in a throwaway branch; keep whichever is green three consecutive runs. Worst case, the window selftest becomes a nightly job and the per-push gate is `--version` only |
| 2 | No GPU on CI runners, so only lavapipe is ever exercised | High | Medium | Accept it. Lavapipe is also the deterministic CPU path we want anyway; real-GPU testing is manual until Phase 5, and `AdapterReport` makes it obvious which one ran |
| 3 | `linuxdeploy` continuous builds change behaviour without notice | Medium | Medium | Pin the downloaded tool by SHA-256 in the workflow and update it deliberately; record the hash in `docs/memory/packaging.md` |
| 4 | glibc 2.35 turns out too new for a user we care about | Low | Medium | The `manylinux_2_28` (glibc 2.28) container is the documented plan B; switching is a one-line change to the container image plus a re-run of the smoke matrix |
| 5 | The `ubuntu-22.04-arm` runner is unavailable or slow | Medium | Low | The aarch64 job is `continue-on-error` in this phase |
| 6 | The zsync `UPDATE_INFORMATION` owner/repo string is wrong, so delta updates silently never work | Medium | Low now, High at release | Derive it from `${{ github.repository }}`; assert with `readelf -p .upd_info` in CI (criterion 14) |
| 7 | An agent copies a `.xar` file into the repository "to make tests simpler", breaking the clean-room rule | Medium | **Severe** — it is a licence violation | `.gitignore` plus a CI check (criterion 19); the rule is restated in the fixture's own doc comment |
| 8 | `panic = "unwind"` plus `lto = "fat"` in the `dist` profile inflates the binary past the budget | Low | Low | Size gate catches it; `strip = "debuginfo"` in `release`, `debug = 1` only in `dist` |
| 9 | Pinning `winit` to a `0.31` beta now would couple Phase 0 to an unstable API | Medium | Medium | Use stable `winit` here; the tablet-driven version question belongs to Phase 5 (`10-architecture.md §7`) |
| 10 | Bundling fontconfig or Mesa by accident makes the app ignore user fonts or crash on other GPUs | Medium | High | Explicit `--exclude-library` list; a CI step lists `AppDir/usr/lib/*.so*` and fails if any name matches the exclusion patterns |

## Test plan

**Unit.** `xarast-testkit` has real tests of its own: the corpus lock parser
(including a deliberately corrupted lock), the golden comparator (identical,
one-pixel-different, size-mismatch, missing-golden), and the budget helper
(under, over, exactly at the limit).

**Integration.** The layering test (`cargo metadata` based). The
desktop-entry/`APP_ID` consistency test. A test that parses
`packaging/linux/xarast.xml` and asserts the MIME type strings match the
constants used in code.

**Corpus.** With the corpus present: all 59 files exist, sizes and hashes match
`corpus.lock`, and the total is 59 — no more, no less. This is the test that
would catch someone pointing the fixture at all 73 `.xar` files in the fork.

**Packaging.** `desktop-file-validate`, `appstreamcli validate --no-net`, the
AppDir library-exclusion check, the size check, the `.upd_info` check.

**Smoke.** The three-distro matrix, each running `--version` and
`--selftest-window`. Both invocations use `--appimage-extract-and-run`.

**Fuzz.** `fuzz_smoke` for 10 000 runs in the per-push gate (cheap, proves the
toolchain), 30 minutes nightly.

**Manual, once, recorded in the memory note.** Run the AppImage on a real
Wayland session under GNOME and under KDE: confirm the window appears, the
taskbar icon is ours (this is what validates the `app_id` work), fractional
scaling does not produce a blurry or misplaced surface, and closing the window
exits the process with status 0.

## Memory note

Create and fill **`docs/memory/packaging.md`** from the template in
`docs/memory/INDEX.md`, recording:

- **Current state.** The workspace layout, the 13 + 1 crates, the binaries each
  produces, and the CI workflow names.
- **Decisions taken (and why).** `linuxdeploy` over `appimage-builder` and
  `cargo-appimage`, with the reasoning above. glibc 2.35 via an explicit
  `ubuntu:22.04` container. `panic = "unwind"` instead of the `abort` currently
  in `Cargo.toml`. Stable `winit` now, beta deferred to Phase 5. The MIME string
  `application/vnd.xarast+zip` from `research/06`, overriding `research/05`.
  Absolute performance budgets rather than criterion's relative comparison. The
  resolved zsync owner/repo string. The pinned `linuxdeploy` SHA-256. Whichever
  of Weston-headless or Xvfb the window selftest ended up using, and why.
- **Invariants that must not be broken.** The Wayland `app_id` equals the
  desktop-entry basename. Nothing at or below `xarast-app` may depend on a UI
  toolkit. No `.xar` file is ever committed to this repository. Hardware and
  compositor libraries are never bundled. Every AppImage invocation in CI uses
  `--appimage-extract-and-run`.
- **Dead ends (do not retry).** musl linking (breaks `dlopen` of GL/Vulkan
  drivers). Bundling fontconfig or freetype (the user stops seeing their fonts).
  Nightly-only `rustfmt` options with a stable CI toolchain.
- **Open TODOs.** aarch64 job promotion from `continue-on-error` to required
  (Phase 12). First-run MIME registration into `~/.local/share` (Phase 12).
  Flatpak as the second channel (Phase 12). The exact/perceptual golden gate
  question (Phase 3/4). Real icon artwork (Phase 12).

Also add a one-line "Phase 0 closed, see packaging.md" entry to
`docs/memory/perf.md` with the two measured numbers (cold start, AppImage size),
creating that note from the template.
