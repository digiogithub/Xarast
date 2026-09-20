# Phase 12 — v0.1 release

> After this phase Xarast stops being "working code that can be built" and becomes a
> signed, updatable, documented, accessible product that a stranger can download, run,
> understand and file a useful bug against — with every performance budget enforced by CI
> rather than asserted in a document.

## Goal

Turn the output of phases 5–11 into **v0.1**: one downloadable, GPG-signed, zsync-updatable
x86_64 AppImage, whose performance is measured and gated, whose licence provenance is proven,
which starts in safe mode after a crash, which can be driven entirely from the keyboard, whose
strings live outside the code, and which ships with user documentation and a first-run
experience.

No new drawing features. Every task in this phase either **measures**, **hardens**,
**documents**, **packages** or **proves** something about what already exists.

## Scope

### In scope

1. **Performance budgets as failing CI gates.** All budgets from `docs/phases/00-roadmap.md`
   plus the ones added here, measured by `xarast-cli` under `criterion`, compared against
   committed baselines, failing the build on regression.
2. **Memory profiling and ceilings.** Peak RSS and allocation profiles for the corpus; hard
   budgets for the bitmap/render cache and the undo log; a leak gate.
3. **Startup time.** Cold start to first presented frame ≤ 400 ms, measured, with a phase
   breakdown emitted by the binary itself.
4. **Accessibility pass.** AccessKit tree exposed for every panel, complete keyboard-only
   operation, visible focus, contrast-checked themes, reduced-motion and system-theme
   following.
5. **i18n/l10n scaffolding.** All user-visible strings extracted to Fluent resources, an
   English-only shipped catalogue, a pseudolocale, and a CI lint that fails on a literal
   user-visible string in code.
6. **Crash reporting and safe mode.** Panic and GPU-loss handlers that write a local report,
   recover autosaved documents, and start the next session in a reduced mode; crash-loop
   detection.
7. **User documentation and first-run.** An mdBook user manual, a generated keyboard
   reference, a man page, AppStream metainfo, and a first-run experience with original sample
   documents (never Xara's).
8. **Release engineering.** Version and changelog policy, a mechanical release checklist,
   AppImage GPG signing, zsync delta updates and an in-app update check.
9. **Licence and provenance audit.** `cargo deny`, `cargo about`, an SBOM, and a clean-room
   audit proving no GPL contamination and no redistributed Xara assets
   (`docs/11-licensing-and-clean-room.md`).
10. **Beta feedback loop.** A public beta channel, issue templates, an in-app problem reporter
    with environment capture, and a documented triage and promotion rule.

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner |
|---|---|
| Shadow, feather, bevel, contour, blend, mould, fractal/noise fills, advanced transparency blend modes | **Phase 13** |
| Windows and macOS shells, MSI/`.app`, notarisation, code signing on those platforms | **Phase 14** |
| Flatpak/Flathub as a second channel, aarch64 AppImage | **Phase 15** (Flatpak may start earlier as a spike, but is not a v0.1 gate) |
| EPS/AI/PDF/EMF/CDR import, print pipeline, CMYK and colour management | **Phase 15** |
| Translating the UI into any language other than English | Post-1.0. This phase ships only the framework and `en-US`. |
| Telemetry or any automatic upload of crash reports | Not planned. Reports stay on disk; the user attaches them manually. Revisit only with an explicit opt-in design post-1.0. |
| Any new tool, node type, filter or file-format feature | — (a change that adds one is out of phase by definition) |

## Prerequisites

- Phases 5–11 closed against the gates in `docs/phases/00-roadmap.md` §Phase gates.
- The 59-file `.xar` corpus parses clean (Phase 3) and renders (Phases 4–5).
- `.xarast` round-trips including unknown-data preservation (Phase 6).
- Export filters for SVG, PNG, JPEG, WebP, PDF exist (Phase 11).
- CI already produces an unsigned AppImage on every push (Phase 0) and runs
  `cargo deny check licenses` (Phase 0).
- A CI **performance reference machine** exists: one dedicated, pinned, non-shared runner.
  Budget gates are meaningless on shared GitHub-hosted runners; securing this machine is a
  Phase 12 prerequisite and blocks workstream A.

## Workstreams

### A. Performance budgets as CI gates

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| A1 | `bench` subcommand: run a named scenario headlessly against a corpus document, emit JSON (`wall_ms`, `p50/p95/p99 frame_ms`, `peak_rss`, `alloc_bytes`) | `xarast-cli` | M | — |
| A2 | Scenario set: pan, zoom, open, undo/redo, save, export, first-paint, for 1k/10k/100k-object documents | `xarast-cli` | M | A1 |
| A3 | `criterion` harness wired to the scenarios, with baselines committed under `benches/baselines/` | workspace | M | A2 |
| A4 | `cargo xtask bench --check-budgets`: compare against `perf-budgets.toml`, exit non-zero on breach | `xtask` (new) | M | A3 |
| A5 | Dedicated perf runner: pinned CPU governor, no other jobs, documented hardware ID | CI | M | — |
| A6 | Noise control: N=11 repetitions, median comparison, ±7 % tolerance band, 3-consecutive-failure rule before hard fail | `xtask` | M | A4, A5 |
| A7 | Perf dashboard artefact: per-commit history published as a static page from CI | CI | S | A4 |
| A8 | Wire the gate into the required checks for merges to the working branch | CI | S | A6 |

The tricky part is **not** measuring, it is making the measurement trustworthy enough to block
a merge. Three rules make it workable. First, the frame-time budget is a **percentile, not a
mean**: pan/zoom at 100k objects is gated on p95 ≤ 16 ms, because a 60 fps claim that is true
on average and false every twelfth frame is a false claim. Second, the comparison is against a
**committed baseline file**, not against "the previous run", so a slow drift of 2 % per commit
cannot walk past the gate. Third, a breach is reported as a **three-strike** condition: one
red run posts a warning comment, three consecutive red runs on the same scenario fail the
build. Without the third rule the gate gets disabled within a month, which is the common
failure mode of performance CI.

Allocation counts are recorded alongside times because they are far less noisy than wall time
and usually identify the regression faster: a scenario whose `alloc_bytes` jumped 40 % while
its time stayed flat is a regression that has not surfaced yet.

### B. Memory profiling and ceilings

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| B1 | `dhat`-based allocation profiling feature (`--features profile-alloc`) on `xarast-cli` | `xarast-cli` | S | A1 |
| B2 | Peak-RSS measurement per scenario, recorded in the same JSON as A1 | `xarast-cli` | S | A1 |
| B3 | Render/bitmap cache: hard byte ceiling, LRU-by-cost eviction, ceiling exposed as a preference and defaulted from available RAM | `xarast-render` | M | — |
| B4 | Undo log: budget in **bytes** not steps, with checkpoint thinning; preference + default | `xarast-doc` | M | — |
| B5 | Leak gate: open→edit→close 200 documents in one process; RSS after the last close within 5 % of after the first | `xarast-cli` | M | B2 |
| B6 | Soak scenario: 60-minute scripted editing session; RSS growth < 5 % over the last 45 minutes | `xarast-cli` | M | B5 |
| B7 | Document the memory model (who owns what, what is cached, what is bounded) | `docs/memory/perf.md` | S | B3, B4 |

`xarast-doc` holds `Arc`-shared heavy payloads (paths, bitmaps, ramps, font data), so "leak"
here almost never means an unreachable allocation — it means a **cache or history entry that
is never evicted**. That is why B5 measures across document *close*, not across edits: a
closed document whose bitmaps are still in the render cache is the exact bug this gate exists
to catch. Expect the first run of B5 to fail, and expect the cause to be cache keys that
reference nodes by id without a document generation in the key.

### C. Startup time

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| C1 | `XARAST_TRACE_STARTUP=1`: emit a phase breakdown (process entry, adapter selection, window mapped, first frame presented) in ms to stderr | `xarast-shell` | S | — |
| C2 | Startup gate: median of 20 cold runs ≤ 400 ms to first presented frame, in CI on the perf runner | `xtask` | S | C1, A5 |
| C3 | Defer font enumeration off the startup path (enumerate on first text/gallery use, with a progress affordance) | `xarast-text`, `xarast-ui` | M | C1 |
| C4 | Defer gallery/thumbnail population to an idle task | `xarast-ui` | M | C1 |
| C5 | Budget `Instance::request_adapter` and cache the adapter decision in the preferences, re-validated on GPU change | `xarast-shell` | M | C1 |
| C6 | Measure AppImage first-run overhead (squashfs mount + page-in) separately and report both cold-cold and warm-cache numbers | CI | S | C1 |

Two honest caveats belong in the measurement. The AppImage's **first** launch on a machine
pays for the FUSE mount and for paging the binary in from disk; that is a real user experience
and must be reported, but the 400 ms budget is defined against a warm page cache, because
otherwise the gate measures the runner's disk. And `request_adapter` on a cold Vulkan ICD can
dominate everything else; if C5 shows it exceeding ~120 ms, the fallback is to present the
window before the GPU is ready and paint the first frame with `vello_cpu`, which is a
behaviour change and must be recorded in `docs/memory/ui.md`.

### D. Accessibility pass

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| D1 | AccessKit adapter wired through `accesskit_winit`; the egui tree is exposed with roles and labels | `xarast-shell`, `xarast-ui` | M | — |
| D2 | Accessible names/descriptions for every panel, gallery row, infobar field and toolbar button | `xarast-ui` | L | D1 |
| D3 | The canvas exposes a meaningful node: document name, selection summary, object count — not an unlabelled image | `xarast-ui` | M | D1 |
| D4 | Full keyboard operation: focus order, focus trap rules for dialogs, `F6` panel cycling, Escape semantics, no mouse-only command | `xarast-ui` | L | — |
| D5 | Visible focus indicator meeting 3:1 against both adjacent colours, in both themes | `xarast-ui` | M | D4 |
| D6 | Contrast audit as a **test**: every theme token pair used for text is computed and asserted ≥ 4.5:1 (≥ 3:1 for ≥ 18.66px bold) | `xarast-ui` | M | — |
| D7 | Follow `org.freedesktop.appearance color-scheme` via `ashpd`; honour a reduced-motion preference | `xarast-shell`, `xarast-ui` | S | — |
| D8 | Screen-reader smoke test procedure with Orca on GNOME/Wayland, recorded as a checklist with results | docs | M | D2, D3 |
| D9 | Configurable UI scale independent of the compositor's fractional scale | `xarast-ui` | M | — |

This workstream assumes Phase 5's go/no-go confirmed **egui** (its W1 spike, with `iced` as the
documented fallback). If the fallback was taken, D1–D3 change substantially — `iced` had no
AccessKit integration at the time of that decision — and the first task of this workstream
becomes re-establishing the transport, which must be re-scoped before the rest of D starts.

`research/05 §2.6` already flags that AccessKit on Linux is incomplete for exotic widgets and
mitigates it by keeping critical controls on standard egui widgets. This phase makes that a
rule with teeth: **a custom-painted widget that carries information or accepts input must
either register an AccessKit node or have a keyboard-reachable standard-widget equivalent.**
The on-canvas handles (gradient arrows, selection blobs) are the hard case — they are
mouse-first by design and are the single most Xara-defining interaction. The resolution is
not to make the blobs screen-reader navigable, but to guarantee that every operation the blobs
perform is also reachable numerically from the infobar, which is where D4's "no mouse-only
command" rule bites. That mapping is enumerated in a table in the user manual.

Contrast (D6) is written as a unit test over the theme token table rather than as a manual
audit, because themes get edited and audits do not get repeated.

### E. i18n and l10n scaffolding

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| E1 | Adopt **Fluent** (`fluent-bundle` + `fluent-langneg`, loaded via `i18n-embed` with `i18n-embed-fl`); pin versions and clear them through `cargo deny` | `xarast-ui` (new dep) | M | — |
| E2 | `t!("id", arg = value)` macro wrapping the bundle lookup; one `L10n` handle owned by `xarast-app` | `xarast-app`, `xarast-ui` | M | E1 |
| E3 | Move every user-visible string into `crates/xarast-ui/i18n/en-US/*.ftl`, split by area (`menus.ftl`, `tools.ftl`, `dialogs.ftl`, `errors.ftl`, `galleries.ftl`) | `xarast-ui` | L | E2 |
| E4 | `cargo xtask i18n check`: (a) no missing key referenced from code, (b) no unused key, (c) no user-visible literal in UI code | `xtask` | M | E3 |
| E5 | Pseudolocale `en-XA` generated from `en-US` at build time: accented, 40 % expanded, bracketed | `xtask` | M | E3 |
| E6 | Locale-aware number parsing/formatting for the unit fields (decimal separator), driven by the active locale, with unit *names* still translatable | `xarast-app` | M | E2 |
| E7 | `TRANSLATING.md`: how to add a locale, where files go, how to test with `XARAST_LOCALE=` | docs | S | E5 |

**Why Fluent rather than gettext.** Fluent is asymmetric by design: a translation may need a
plural category or a gender distinction the English source does not have, and it expresses
that inside the `.ftl` file without changing call sites. That matters for a program full of
"3 objects selected", "Undo: Move", and unit strings. `gettext` would force `ngettext` plumbing
into every call site for cases English does not need.

**How strings are extracted.** Not by scanning source for string literals — by inversion. The
code references **identifiers only** (`t!("tool-selector-name")`) and the `.ftl` files are the
source of truth. E4 walks the AST of the UI crates collecting `t!` identifiers, diffs them
against the keys in `en-US`, and fails on either direction. The "no literal" check (E4c) is a
lint over the UI crates: any string literal reaching an egui text-taking call must come from
`t!` or be on an explicit allowlist (glyph strings, format scaffolding, debug output). This is
enforceable and mechanical; a scraper of literals is neither.

E5 exists because it is the only cheap way to discover both classes of failure at once: an
untranslated string shows up unaccented, and a layout that cannot survive a 40 % longer German
string breaks visibly. A screenshot pass in `en-XA` is part of the release checklist.

Not covered by `docs/research/*`: this whole workstream is a Phase 12 decision. The crate
choice above is a recommendation to be confirmed in the first week by a spike that checks the
current versions, their licences via `cargo deny`, and their binary-size cost; if `i18n-embed`
does not clear the licence gate, the fallback is to load `.ftl` files from the AppDir directly
and drop the embedding crate.

### F. Crash reporting and safe mode

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F1 | Panic hook: capture backtrace, thread name, build id, GPU adapter, OS/compositor, last 200 log lines (no document content), write to `$XDG_STATE_HOME/xarast/crashes/<ts>.toml` | `xarast-app` | M | — |
| F2 | GPU device-loss and surface-error recovery: rebuild the device, and on a second failure fall back to `vello_cpu` and tell the user | `xarast-shell`, `xarast-render` | L | — |
| F3 | Emergency save: on panic in a non-document thread, write `*.xarast.recovered` for each dirty document before exiting | `xarast-app` | M | F1 |
| F4 | Startup sentinel: a file written at start and removed on clean exit; its presence at next start means the previous session crashed | `xarast-app` | S | F1 |
| F5 | Safe mode (`--safe-mode`, or offered after a crash): CPU renderer, default preferences, no session restore, no gallery scan, plain theme | `xarast-app`, `xarast-shell` | M | F4 |
| F6 | Crash-loop detection: 3 crashes within 10 minutes forces safe mode without asking | `xarast-app` | S | F4 |
| F7 | Recovery dialog on next start: lists recovered documents and the crash report path, with a "copy report" button | `xarast-ui` | M | F3, F4 |
| F8 | `XARAST_FORCE_PANIC=<site>` test hook, compiled in release, to exercise F1–F7 in CI | `xarast-app` | S | F1 |

The report contains **no document content and no file paths outside the document's basename**;
that rule is written in the code comment and in the user manual, because a crash report a user
is afraid to attach is a crash report we never see. Nothing is uploaded: F7 shows the path and
offers to copy it, and the beta issue template (workstream J) asks for it.

F2 deserves emphasis: on Linux/Wayland, GPU device loss from a driver update or a suspend cycle
is not exotic, and the difference between "Xarast survived a driver reset" and "Xarast is the
app that dies when I close the lid" is the entire stability perception of v0.1.

### G. User documentation and first run

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| G1 | mdBook user manual: install, first drawing, tools, fills and transparency, layers, text, import/export, preferences, troubleshooting | `docs/manual/` | L | — |
| G2 | Keyboard reference **generated** from the command registry at build time, so it cannot drift | `xtask` | M | — |
| G3 | First-run experience: a welcome screen with "new document / open / sample documents / manual", shown once, dismissible forever | `xarast-ui` | M | — |
| G4 | 5–8 original sample documents authored for Xarast (never Xara's `testfiles/` or `Designs/`), each demonstrating one capability | `assets/samples/` | M | — |
| G5 | AppStream metainfo with description, screenshots, release notes; `desktop-file-validate` and `appstreamcli validate` clean | `packaging/linux/` | S | — |
| G6 | `xarast(1)` man page and `xarast-cli` `--help` covering every subcommand | `xarast-cli` | S | — |
| G7 | Troubleshooting chapter documenting `XARAST_RENDERER`, `WGPU_BACKEND`, `--safe-mode`, crash report location, log location | docs | S | F5 |
| G8 | Help menu linking to the manual shipped inside the AppImage (offline-first), with an online fallback | `xarast-ui` | S | G1 |

G4 is a licence requirement, not a nicety: `docs/11-licensing-and-clean-room.md` §3.2 forbids
redistributing Xara's corpus. The samples are authored from scratch, their authorship recorded
in `assets/samples/README.md`, and they are licensed CC0 so users can dissect them freely.

G2 matters more than it looks. `research/04 §4` documents 142 lines of original hotkeys across
eight categories including 24 nudge combinations; a hand-written table for that will be wrong
within two releases. Generating it from the registry also gives the accessibility workstream a
free correctness check: a command with no keyboard binding shows up as a blank cell.

### H. Release engineering: versioning, changelog, signing, updates

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| H1 | Version policy document: `0.MINOR.PATCH` pre-1.0; `.xarast` format version is **independent** of the app version (`research/06 §7.4`) | docs | S | — |
| H2 | Changelog policy: Keep a Changelog format, `CHANGELOG.md` updated in the same PR as the change, `Unreleased` section enforced by a CI check on user-visible changes | CI, docs | M | H1 |
| H3 | Release checklist as an executable script where possible (`cargo xtask release --dry-run`) and a markdown checklist where not | `xtask`, docs | M | H1 |
| H4 | GPG signing of the AppImage (`appimagetool --sign`), public key published, verification documented | CI | M | — |
| H5 | Confirm and complete Phase 0's zsync setup (`UPDATE_INFORMATION=gh-releases-zsync\|…\|Xarast-*-x86_64.AppImage.zsync`): the `.zsync` is uploaded to the **same** release as the AppImage, and an actual delta update is exercised end to end | CI | S | — |
| H6 | In-app update check: read the ELF `.upd_info` section, query the release, offer to run `appimageupdatetool` when present, otherwise open the download page. Opt-out preference, off by default on first run until the user answers | `xarast-app`, `xarast-ui` | M | H5 |
| H7 | Delta-size measurement: build N and N+1, measure the actual zsync transfer, record it | CI | S | H5 |
| H8 | Release notes generated from the changelog into the GitHub release **and** the AppStream metainfo | `xtask` | S | H2, G5 |
| H9 | Reproducibility: pinned Ubuntu 22.04 build container digest, `--locked` builds, recorded toolchain version | CI | M | — |

`research/05 §11.2` fixes the glibc baseline at Ubuntu 22.04 / glibc 2.35 built **inside a
container**, not on the runner; H9 makes that a pinned digest so that a GitHub image refresh
cannot silently move our baseline. The AppImage must also not bundle the exclusion list in
`research/05 §11.3` — especially fontconfig and freetype, whose bundling makes user fonts
invisible. A packaging test (workstream J/K below) asserts the AppDir contains none of them.

### I. Licence and provenance audit

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| I1 | `cargo deny check` (licenses, bans, advisories, sources) green, with every exception justified in `deny.toml` | CI | M | — |
| I2 | `cargo about generate` → `THIRD-PARTY-LICENSES.html` shipped inside the AppDir and linked from Help ▸ About | `xtask` | S | I1 |
| I3 | SBOM in CycloneDX JSON, attached to the release, generated by `cargo cyclonedx` (or equivalent that clears I1) | CI | M | I1 |
| I4 | **GPL/AGPL proof**: assert the dependency graph contains zero GPL/AGPL/SSPL crates; the check fails the build, it does not warn | CI | S | I1 |
| I5 | **Xara-asset proof**: assert no file under `assets/`, `packaging/` or the built AppDir originates from the original tree; assert the corpus under `tests/corpus-xar/` is `.gitignore`d and absent from the AppImage | CI | M | G4 |
| I6 | String audit: every occurrence of "Xara" in shipped strings and metadata is nominative and truthful ("imports Xara Xtreme files"); listed and reviewed | `xtask`, docs | S | E3 |
| I7 | Clean-room attestation: a dated record confirming each shipped crate was implemented from `docs/research/*`, with the reviewer named | `docs/11-licensing-and-clean-room.md` addendum | M | — |
| I8 | `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE` and the trademark disclaimer present, and `license = "MIT OR Apache-2.0"` set on every workspace crate | workspace | S | — |
| I9 | MPL-2.0 watch items (`resvg`/`usvg` if used) recorded with justification; confirm no modifications were made to MPL files, or publish them if any were | `deny.toml`, docs | S | I1 |

This is the gate that must never be waived, because `docs/11-licensing-and-clean-room.md` §2.3
makes the permissive licence conditional on Xarast not being a derivative work. I5 in
particular is an automated check and not a promise: the corpus lives outside the repository or
behind `.gitignore`, and the packaging job greps the assembled AppDir for any file hash that
matches one in `/home/user/xara-xtreme`. Note also that `research/05` still states
GPL-3.0-or-later in its DECISIONS block — that is **superseded** by
`docs/11-licensing-and-clean-room.md`, and I8 is where the workspace manifests are confirmed
to reflect the current decision.

### J. Beta feedback loop

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| J1 | Beta channel: pre-releases tagged `v0.1.0-beta.N`, published with their own zsync, announced with known-issues text | CI, docs | S | H5 |
| J2 | Issue templates: bug (with environment block), rendering defect (requires a `.xarast` and an expected/actual image), performance report (requires a `bench` JSON) | `.github/` | S | — |
| J3 | Help ▸ Report a problem: opens the browser at a prefilled issue URL with build id, GPU adapter, backend tier, compositor, locale, and the crash report path — content copied to clipboard, never uploaded | `xarast-ui` | M | F1, J2 |
| J4 | `xarast-cli diagnose`: dumps the same environment block for users who cannot start the GUI | `xarast-cli` | S | J3 |
| J5 | Triage policy: labels, severity definitions, a weekly triage pass, and a published response-time expectation | docs | S | J2 |
| J6 | Beta → release rule, stated numerically (see acceptance criterion 16) | docs | S | J1 |

## Public API introduced

```rust
// xarast-app
pub struct L10n { /* … */ }
impl L10n {
    pub fn load(requested: &[LanguageIdentifier]) -> Self;
    pub fn get(&self, id: &str) -> Cow<'_, str>;
    pub fn get_args(&self, id: &str, args: &FluentArgs) -> Cow<'_, str>;
    pub fn active(&self) -> &LanguageIdentifier;
}
#[macro_export] macro_rules! t { /* t!("id") | t!("id", name = value) */ }

pub struct CrashReport { /* build, gpu, os, backtrace, log_tail */ }
impl CrashReport {
    pub fn install_hooks(dirs: &AppDirs);
    pub fn write(&self, dir: &Path) -> io::Result<PathBuf>;
}

pub enum StartupMode { Normal, Safe { reason: SafeModeReason } }
pub enum SafeModeReason { UserRequested, PreviousCrash, CrashLoop }
pub fn decide_startup_mode(dirs: &AppDirs, args: &Args) -> StartupMode;

pub struct SessionRecovery { pub documents: Vec<RecoveredDocument> }
pub fn scan_recovery(dirs: &AppDirs) -> SessionRecovery;

// xarast-render
pub struct CacheBudget { pub max_bytes: u64 }
impl RenderCache { pub fn set_budget(&mut self, b: CacheBudget); pub fn stats(&self) -> CacheStats; }
pub struct CacheStats { pub bytes: u64, pub entries: usize, pub hits: u64, pub misses: u64, pub evictions: u64 }

// xarast-doc
pub struct HistoryBudget { pub max_bytes: u64 }
impl History { pub fn set_budget(&mut self, b: HistoryBudget); pub fn bytes(&self) -> u64; }

// xarast-shell
pub struct StartupTrace { pub phases: Vec<(&'static str, Duration)> }
pub fn startup_trace() -> Option<&'static StartupTrace>;
pub enum RendererTier { Gpu, Hybrid, CpuComposited, Software }
pub fn selected_tier() -> RendererTier;

// xarast-cli
// xarast-cli bench   --scenario <name> --doc <file> --repeat <n> --json <out>
// xarast-cli soak    --doc <file> --minutes <n> --report <out>
// xarast-cli diagnose [--json]
```

No new public API is added to `xarast-geom`, `xarast-color`, `xarast-xar`, `xarast-format`,
`xarast-io` or `xarast-text` in this phase. A pull request in Phase 12 that widens one of those
APIs is out of phase.

## Acceptance criteria

Each is a command or a measured number. All are run on the pinned perf runner unless stated.

1. `cargo xtask bench --check-budgets` exits 0, having evaluated **every** row of the
   Performance budgets table below, with N=11 repetitions each.
2. `cargo xtask bench --scenario pan --doc corpus/100k.xarast` reports **p95 frame time
   ≤ 16 ms** and p99 ≤ 24 ms on the reference machine's integrated GPU.
3. `cargo xtask bench --scenario open --doc corpus/5mb.xar` reports **≤ 500 ms** to first paint
   (median of 11).
4. `cargo xtask bench --scenario startup` reports **≤ 400 ms** median (of 20 cold runs, warm
   page cache) from process entry to first presented frame, and the phase breakdown is present
   in the JSON.
5. `cargo xtask soak --doc corpus/large.xarast --minutes 60` reports RSS growth **< 5 %** over
   the final 45 minutes and zero panics.
6. `cargo xtask leakcheck --iterations 200` reports final RSS within **5 %** of the RSS after
   the first document close.
7. `cargo nextest run -p xarast-ui accessibility` passes, including the contrast test asserting
   **≥ 4.5:1** for every text token pair in both themes and **≥ 3:1** for the focus indicator.
8. `cargo nextest run -p xarast-ui keyboard_only` passes: a scripted `egui_kittest` session
   creates a document, draws a rectangle, applies a gradient fill, renames a layer, saves and
   exports — using keyboard events only.
9. The Orca screen-reader checklist in `docs/manual/accessibility.md` is completed with every
   item marked pass or with a filed issue, signed and dated.
10. `cargo xtask i18n check` reports **0** missing keys, **0** unused keys and **0**
    unauthorised user-visible literals.
11. `XARAST_LOCALE=en-XA` starts, and the pseudolocale screenshot pass shows no clipped or
    overlapping text in any panel at 1280×800.
12. `XARAST_FORCE_PANIC=render` produces a report file under
    `$XDG_STATE_HOME/xarast/crashes/`, writes a `.xarast.recovered` file for the dirty
    document, and the next start offers recovery and safe mode; asserted by
    `cargo nextest run -p xarast-app crash_recovery`.
13. Three forced crashes within 10 minutes cause the fourth start to enter safe mode without
    prompting (same test module).
14. `cargo deny check` exits 0 for licenses, bans, advisories and sources; `cargo xtask
    audit-gpl` reports **0** GPL/AGPL/SSPL crates; the CycloneDX SBOM is attached to the
    release; `cargo xtask audit-assets` reports **0** files in the AppDir matching a hash from
    the original tree and confirms the `.xar` corpus is absent.
15. The release artefacts verify: `gpg --verify Xarast-0.1.0-x86_64.AppImage.sig`,
    `appstreamcli validate` and `desktop-file-validate` all exit 0; measured zsync delta
    between `0.1.0-beta.N` and `0.1.0` is **≤ 15 MB** and recorded.
16. Beta promotion rule met: **≥ 20** distinct external testers on `v0.1.0-beta.*`, **zero**
    open issues labelled `severity:crash` or `severity:data-loss`, and **≥ 14 days** since the
    last beta with no new crash report.
17. The AppImage runs on clean containers of Ubuntu 22.04, Ubuntu 24.04, Debian 12, Fedora 41
    and Arch (current) — GNOME/Wayland and KDE/Wayland — opening a `.xar` from the corpus and
    exporting a PNG; `cargo xtask test-distros` reports all green.
18. AppImage size **≤ 80 MB** and it bundles none of the libraries on the
    `research/05 §11.3` exclusion list; asserted by `cargo xtask audit-appdir`.
19. `mdbook build docs/manual` succeeds, and `cargo xtask docs check-commands` reports every
    registered command present in the generated keyboard reference.
20. First run on a clean `$XDG_CONFIG_HOME` shows the welcome screen, opens a sample document,
    and never shows the welcome screen again after dismissal (tested by `xarast-app`
    `first_run` integration test).

## Performance budgets

| Budget | Target | Gate | Notes |
|---|---|---|---|
| Pan/zoom frame time, 100k objects, integrated GPU | **p95 ≤ 16 ms**, p99 ≤ 24 ms | A4 | Tightens the roadmap's "≤ 16 ms" into a percentile |
| Open a 5 MB `.xar` | ≤ 500 ms to first paint | A4 | From the roadmap |
| Undo/redo of a single edit | ≤ 1 ms | A4 | From the roadmap |
| Save a 20 MB `.xarast` | ≤ 1 s | A4 | From the roadmap |
| Cold start to first frame | ≤ 400 ms | C2 | From the roadmap; warm page cache |
| AppImage size | ≤ 80 MB | A4/I | From the roadmap |
| Peak RSS, 100k-object document open + one pan pass | ≤ 1.5 GB | A4 | **New in this phase.** Confirm against measurement in week 1; if the measured floor exceeds it, raise it once, with the number recorded in `docs/memory/perf.md` |
| Idle CPU, window visible, no interaction | < 1 % of one core over 60 s | A4 | **New.** Catches a redraw loop, the most common battery complaint |
| Render cache ceiling honoured | ≤ configured bytes + 5 % | B3 | **New** |
| Undo history ceiling honoured | ≤ configured bytes + 5 % | B4 | **New** |
| Export a 4000×4000 PNG of a 10k-object document | ≤ 3 s | A4 | **New.** To be calibrated in week 1 from the Phase 11 measurement |
| zsync delta, patch release | ≤ 15 MB transferred | H7 | **New** |
| Time to first interaction after opening the largest corpus file | ≤ 2 s | A4 | **New.** "First paint" is not the same as "usable"; both are measured |

Budgets marked "to be calibrated in week 1" are set by measuring the current build, adding a
20 % headroom, and committing the number. That is how a budget stays a gate instead of an
aspiration.

## Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Perf gates are noisy and get disabled | **High** | High | Dedicated pinned runner (A5); percentile not mean; three-strike rule (A6); allocation counts as a low-noise corroborating signal |
| Some budget cannot be met and blocks the release | Medium | High | Measure all budgets in **week 1**, not at the end. Any budget found unreachable is either fixed with a scoped optimisation task inside this phase or formally re-set once, with the justification and the number recorded in `docs/memory/perf.md`. Silently dropping a budget is not an option |
| String extraction (E3) is a huge mechanical diff that collides with other work | High | Medium | Do E3 as one atomic PR merged at the start of the phase, before D2 and G1 touch the same files |
| AccessKit on Linux is incomplete for our custom widgets | Medium | Medium | `research/05 §2.6`'s mitigation, made a rule in D: custom widget ⇒ AccessKit node or keyboard-reachable standard equivalent; canvas handles covered by the infobar-equivalence table |
| GPU device loss handling (F2) is hard to test | Medium | Medium | Test with `WGPU_BACKEND=gl` plus a forced device-loss hook (F8); additionally soak under a driver-reset script on the reference machine |
| AppImage works on the build distro and fails elsewhere | Medium | High | Criterion 17's five-distro container matrix runs on every release candidate, not just at the end |
| GPG key management (H4) becomes a single point of failure | Low | High | Key generated for the project, private part in CI secrets **and** in offline backup; a documented rotation procedure; the public key is published in the repository and in the release body |
| A dependency changes licence between releases | Low | High | `cargo deny` runs on every build, not only at release; `--locked` builds pin exactly what was audited |
| Beta testers never materialise, so criterion 16 blocks forever | Medium | Medium | Recruit during Phase 11 (post to Linux graphics and vector-art communities with the nightly AppImage); if after 30 days fewer than 20 testers have appeared, release as `v0.1.0` with the tester count recorded and the criterion relaxed **once**, in writing |
| Update check (H6) is perceived as phone-home | Low | Medium | Off until explicitly answered on first run; documented exactly what is sent (a version query, nothing else); a build-time flag to compile it out for downstream packagers |

## Test plan

**Automated, every PR**
- `cargo nextest run --workspace`, `cargo clippy --workspace -- -D warnings`,
  `cargo deny check`, `cargo xtask i18n check`, `cargo xtask audit-gpl`.
- Golden-image tests on the deterministic `vello_cpu` path, exact match, per
  `research/05 §13.1` level A.
- `egui_kittest` UI snapshots including the keyboard-only session (criterion 8) and the
  accessibility tree snapshot for each panel.
- Contrast test (D6) and the generated-keyboard-reference check (G2).

**Automated, every merge to the working branch**
- `cargo xtask bench --check-budgets` on the perf runner, with results appended to the history
  artefact.
- AppImage build, `audit-appdir`, `audit-assets`, size check.

**Nightly**
- GPU↔CPU parity on the lavapipe runner (`research/05 §13.1` level B: ΔRMS < 0.5 %, no pixel
  Δ > 8/255).
- Fuzz targets 30 min each: `fuzz_xar_parse`, `fuzz_xarast_parse`, `fuzz_svg_import`,
  `fuzz_path_boolean`, `fuzz_text_shape`.
- `soak --minutes 60` and `leakcheck --iterations 200`.
- Five-distro container matrix (criterion 17).

**Per release candidate (manual, checklisted)**
- Orca screen-reader pass on GNOME/Wayland (D8).
- `en-XA` pseudolocale screenshot pass across all panels and dialogs.
- Fresh-user run: clean `$XDG_CONFIG_HOME`, download, `chmod +x`, run, first-run flow, open a
  sample, export a PNG, quit — timed and recorded.
- Update path: install `beta.N`, run `appimageupdatetool`, confirm it lands on the release
  build and the delta size matches H7.
- Signature verification from a machine that has never built the project.
- Crash-and-recover drill: kill the process mid-edit with `SIGKILL`, confirm recovery offers
  the document and that no data beyond the last autosave interval is lost.

**Explicitly manual and recorded, not automated**
- Tablet pressure on real hardware (one Wacom, one Huion) under GNOME and KDE Wayland.
- Fractional scaling at 125 %, 150 % and 175 %.
- Multi-monitor with mixed scale factors.

## Memory note

On close, update:

- **`docs/memory/perf.md`** — the final budget table with measured numbers and the reference
  machine's identity; every budget that was re-set, with the reason; the noise characteristics
  of each scenario (so the next agent knows which ones are flaky); the memory ownership model
  from B7; cache and history default ceilings and how they are derived from RAM.
- **`docs/memory/packaging.md`** — the pinned container digest, the exclusion list as
  actually applied, the signing and key-rotation procedure, the zsync configuration and the
  measured delta, the five-distro results, and every distro-specific workaround found.
- **`docs/memory/ui.md`** — the accessibility decisions (which widgets carry AccessKit nodes,
  the infobar-equivalence table for canvas handles), the focus-order rules, the theme token
  table and its contrast results, the i18n architecture and the `t!` convention, the
  first-run flow, and whether C5 forced presenting the window before the GPU was ready.
- **`docs/11-licensing-and-clean-room.md`** — append the dated clean-room attestation (I7),
  the SBOM location, and any `deny.toml` exception added with its justification.
- **`docs/memory/INDEX.md`** — add a `release.md` note covering the version and changelog
  policy, the release checklist, the beta promotion rule and the triage policy, and register
  it in the index table.
