# Phase 14 — Windows and macOS

> After this phase Xarast is a program you can install on Windows 10/11 and macOS 12+ without
> a security warning, with working tablet pressure, system fonts, native file dialogs and file
> associations — and CI produces signed artefacts for all three platforms from the same commit.

## Goal

Port the shell, not the product. Everything below `xarast-app` already builds without a
windowing system (`docs/10-architecture.md` §2), so this phase is about the three things that
are genuinely platform-specific — **surface and GPU backend, input devices, and system
services** — plus the disproportionately large work of **packaging, signing and distribution**
on two platforms that both actively punish unsigned software.

This phase runs in parallel with Phase 13 (`docs/phases/00-roadmap.md`). The two must not
collide: Phase 13 touches `xarast-doc`, `xarast-geom`, `xarast-render` and `xarast-ui`;
Phase 14 touches `xarast-shell`, `packaging/` and CI, and only reads the others.

## Scope

### In scope

1. An explicit audit of `xarast-shell`: what is platform-specific, what is not, and a trait
   boundary that makes the difference visible in the type system rather than in `cfg` blocks
   scattered through the file.
2. **Windows**: GPU backend choice and tiering, Windows Ink tablet input, system font
   enumeration, native file dialogs and portals-equivalents, clipboard and drag-and-drop, file
   associations, MSI and portable builds, code signing, per-monitor DPI.
3. **macOS**: Metal backend, `NSEvent` tablet input implemented by hand, CoreText font
   enumeration, menu-bar and shortcut conventions, `.app` bundle in a `.dmg`, universal
   binary, hardened runtime, notarisation and stapling, and an honest answer on sandboxing.
4. A three-platform CI matrix with signing secrets, artefact naming, and the golden/parity
   test strategy adapted to each platform's GPU reality.
5. A platform-specific manual test plan and a published, honest list of what is second-class.

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner / reason |
|---|---|
| Any new drawing feature, tool, node type or file-format capability | Phase 13 / Phase 15. A PR in this phase that changes `xarast-doc` or `xarast-geom` is out of phase unless it is fixing a portability bug |
| Windows on ARM (`aarch64-pc-windows-msvc`) | **Phase 15** or later. `research/05 §12.1` puts it in a later phase; we cross-compile nothing we cannot test |
| Linux aarch64 AppImage | **Phase 15** |
| Mac App Store distribution and full App Sandbox | **Not planned for 1.0.** See the sandbox discussion below |
| Windows Store / MSIX | Not planned. Winget manifest only (`research/05 §12.1`) |
| Wintab as the primary tablet path on Windows | Plan B only (`research/05 §12.1`); adopted only if Ink fails on real hardware |
| Printing on either platform | **Phase 15** owns print end to end |
| Platform-native UI look (Fluent/AppKit widgets) | Never. `egui` draws our UI on all three platforms; only the *window frame* and system dialogs are native |
| Auto-update on Windows/macOS beyond "check and open the download page" | Phase 15 at the earliest; the in-app check from Phase 12 H6 is reused, the installer download is manual |

## Prerequisites

- **Phase 12 closed.** The Linux release exists, budgets are gated, the licence audit passes,
  and crash reporting and safe mode work — all of which this phase now has to make work twice
  more.
- `xarast-shell` is the only crate that touches `winit`, `wgpu` surfaces, clipboard, file
  dialogs, tablets and accessibility adapters. If anything else does, fixing that is task B0
  and it blocks everything else.
- The CPU render path (`vello_cpu`) is the deterministic oracle and works with no GPU at all
  — this is what makes CI testable on runners that have no usable GPU.
- Apple Developer Program membership (99 USD/yr) and an Azure Trusted Signing subscription
  (or an EV certificate) are **procurement prerequisites**, not tasks. Start both before the
  phase opens; the Apple enrolment in particular can take days.

## Workstreams

### A. What is platform-specific, and what is not

Before any porting, the boundary is written down. This table is the phase's contract and
belongs in `docs/memory/packaging.md` at close.

| Concern | Platform-specific? | Where it lives |
|---|---|---|
| Document model, geometry, colour, text layout, `.xar`, `.xarast`, filters, tools, commands, undo | **No** | `xarast-geom/-color/-doc/-text/-image/-render/-xar/-format/-io/-app` — must compile and pass tests on all three with no `cfg(target_os)` |
| Panels, galleries, dialogs, canvas widget, on-canvas handles | **No** | `xarast-ui`; `egui` is the same everywhere |
| Window creation, decorations, minimise/maximise/close semantics | Partly | `winit`, abstracted in `xarast-shell` |
| GPU surface creation and backend selection | **Yes** | `xarast-shell::gpu` |
| Tablet/stylus input | **Yes** — three different sources | `xarast-shell::input` behind `TabletSource` |
| Keyboard modifiers and shortcut defaults | **Yes** (Ctrl vs Cmd, F-keys) | `xarast-app::keymap` with a per-platform default table |
| System font enumeration | Partly (`fontique` abstracts it, its behaviour differs) | `xarast-text` + a shell-provided hook |
| File dialogs | **Yes** | `rfd` behind `xarast-shell::dialogs` |
| Clipboard (text, image, and later SVG/EMF flavours) | **Yes** | `arboard` behind `xarast-shell::clipboard` |
| Drag and drop of files into the window | **Yes** | `winit` events, normalised in the shell |
| Standard directories (config, state, cache, documents) | **Yes** | `directories`-style crate behind `xarast-app::AppDirs` |
| Single-instance behaviour and "open with" handoff | **Yes** | `xarast-shell::instance` |
| File associations and icons | **Yes** | `packaging/{linux,windows,macos}` |
| DPI / scaling model | **Yes** (fractional vs per-monitor-v2 vs backing scale) | `xarast-shell::scaling` |
| System theme and accent colour | **Yes** | `xarast-shell::appearance` |
| Accessibility adapter | **Yes** (AT-SPI / UIA / NSAccessibility) | `accesskit_winit` in `xarast-shell` |
| Crash handling and the report path | Partly | `xarast-app`, with per-platform directories |
| Packaging, signing, update mechanism | **Yes** | `packaging/`, CI |

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| A1 | Audit: assert no `cfg(target_os)` outside `xarast-shell`, `xarast-app::AppDirs` and `packaging/`; fix violations | workspace | M | — |
| A2 | Define the shell's platform traits (`GpuSurface`, `TabletSource`, `Dialogs`, `Clipboard`, `Appearance`, `Instance`) with a Linux implementation as the reference | `xarast-shell` | L | A1 |
| A3 | `cargo check --target x86_64-pc-windows-msvc` and `--target aarch64-apple-darwin` green for every crate below `xarast-shell` | CI | M | A1 |
| A4 | Headless test suite (`xarast-cli`, golden images, round trips) runs on all three OSes in CI | CI | M | A3 |
| A5 | Enforcement lint in CI so the boundary does not rot | CI | S | A1 |

A4 is the highest-value early task in the phase and it is cheap: the deterministic CPU path
means the document, geometry, format and render tests can run on Windows and macOS runners
**before any shell work exists**. Doing this first turns "does the core port?" from an open
question into a green checkmark, and it usually surfaces two or three real bugs (path
separators, line endings in text fixtures, `f64` formatting differences, filesystem case
sensitivity on macOS).

### B. Windows

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| B1 | Backend tiering: **DX12 primary**, Vulkan second, GLES (ANGLE/WARP) third, `vello_cpu` fourth; the same four-tier model as `research/05 §4.3` with Windows names | `xarast-shell` | M | A2 |
| B2 | `WGPU_BACKEND` and `XARAST_RENDERER` escape hatches documented for Windows | docs | S | B1 |
| B3 | Windows Ink tablet input via `winit` 0.31 `TabletTool`, normalised to `StrokeSample` | `xarast-shell` | M | A2 |
| B4 | Wintab fallback behind a feature flag and a preference, adopted only if B3 fails on real Wacom/Huion hardware | `xarast-shell` | M | B3 |
| B5 | System fonts via `fontique`'s DirectWrite path; verify user-installed and per-user fonts are visible | `xarast-text` | M | A2 |
| B6 | Native file dialogs via `rfd`, with the recent-files and default-directory behaviour Windows users expect | `xarast-shell` | S | A2 |
| B7 | Clipboard: text, PNG and DIB flavours in and out; SVG flavour if cheap | `xarast-shell` | M | A2 |
| B8 | Per-monitor DPI v2: correct scaling on mixed-DPI setups and on monitor change mid-session | `xarast-shell` | M | A2 |
| B9 | Keymap: Ctrl-based defaults, Windows-conventional Alt menu access, and a conflict audit against `research/04 §4` | `xarast-app` | M | A2 |
| B10 | `crt-static` release profile so no VCRedist install is required | workspace | S | — |
| B11 | MSI via `cargo-packager` (WiX), with file associations for `.xarast` and `.xar`, ProgID, icons, upgrade codes and a clean uninstall | `packaging/windows` | L | B10 |
| B12 | Portable ZIP build: no registry writes, preferences beside the executable when a marker file is present | `packaging/windows`, `xarast-app` | M | B10 |
| B13 | Code signing with **Azure Trusted Signing** in CI; signed EXE, DLLs (if any) and MSI | CI | L | B11 |
| B14 | Crash handling on Windows: report directory under `%LOCALAPPDATA%`, safe mode, crash-loop detection | `xarast-app` | M | — |
| B15 | Winget manifest as a post-release extra | `packaging/windows` | S | B13 |
| B16 | Windows-specific accessibility: verify the AccessKit UIA adapter with Narrator on the same checklist Phase 12 used with Orca | `xarast-shell` | M | A2 |

**Backend choice: DX12 primary, and why not Vulkan.** `wgpu` supports both, and on paper
Vulkan would let one code path serve Windows and Linux. In practice the Windows driver
landscape makes DX12 the safer default: every supported GPU has a maintained DX12 driver,
Vulkan availability on older Intel integrated parts is patchy, and WARP gives a supported
software DX12 device without shipping anything. Vulkan stays as the second tier because it is
occasionally *better* (some AMD setups) and because keeping both exercised protects the Linux
path. The selection is automatic with an environment-variable override, exactly as on Linux.

**Windows Ink is the primary tablet path and the main risk.** `research/05 §3.3` records
winit 0.31 as supporting Ink with force, tilt and twist, and `research/05 §12.1` flags the
known real-world problem that some Wacom and Huion drivers behave better through Wintab than
Ink — the same issue Photoshop documents. The plan is therefore: ship Ink, test on at least
one Wacom and one Huion device before release, and keep `wintab_lite` behind a preference so a
user with a misbehaving driver has a switch rather than a bug report. If B3 fails on both test
devices, B4 is promoted from fallback to primary and that decision is recorded.

**Signing is not optional.** Without a signature SmartScreen blocks downloads outright, and an
ordinary OV certificate has to accumulate reputation before the warnings stop. Azure Trusted
Signing (`research/05 §12.1`) avoids the physical HSM and is the recommended route. Budget for
the possibility that reputation takes a few hundred downloads to build, and say so in the
release notes rather than letting users discover it.

### C. macOS

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| C1 | Metal backend via `wgpu` (no MoltenVK); CPU fallback tier retained | `xarast-shell` | M | A2 |
| C2 | Tablet input via `NSEvent`: `NSEventTypeTabletPoint` / `NSEventSubtypeTabletPoint`, reading pressure, tilt, rotation and tangential pressure, using `objc2` / `objc2-app-kit` | `xarast-shell` | L | A2 |
| C3 | Proximity events (`NSEventTypeTabletProximity`) to distinguish stylus from eraser and to know when the pen leaves the tablet | `xarast-shell` | M | C2 |
| C4 | Font enumeration via `fontique`'s CoreText path; verify user fonts in `~/Library/Fonts` and font collections | `xarast-text` | M | A2 |
| C5 | Menu bar: a real `NSMenu` with the application menu, standard items (About, Preferences…, Services, Hide, Quit) wired to our commands | `xarast-shell` | L | A2 |
| C6 | Keymap: Cmd-based defaults, Option/Alt semantics, an audit of every `research/04 §4` shortcut against macOS system reservations (notably F-keys and Cmd+Space) | `xarast-app` | L | C5 |
| C7 | Backing-scale handling for Retina, and mixed-scale multi-monitor | `xarast-shell` | M | A2 |
| C8 | Clipboard and drag-and-drop with `NSPasteboard` flavours (PNG, PDF, text) | `xarast-shell` | M | A2 |
| C9 | Universal binary: build both targets, `lipo -create`, verify with `lipo -info` and `file` | CI | M | — |
| C10 | `.app` bundle with `Info.plist` (document types, UTIs for `.xarast` and `.xar`, `NSHighResolutionCapable`), icon set, and a `.dmg` with a background and Applications symlink | `packaging/macos` | L | C9 |
| C11 | Hardened runtime and `codesign --force --options runtime --timestamp` with a Developer ID Application certificate | CI | M | C10 |
| C12 | Notarisation (`xcrun notarytool submit --wait`) and stapling (`xcrun stapler staple`), with secrets in CI | CI | L | C11 |
| C13 | Crash handling on macOS: reports under `~/Library/Application Support/Xarast/crashes`, safe mode, crash-loop detection | `xarast-app` | M | — |
| C14 | macOS accessibility: verify the AccessKit NSAccessibility adapter with VoiceOver on the Phase 12 checklist | `xarast-shell` | M | A2 |
| C15 | `MACOSX_DEPLOYMENT_TARGET=11.0` set and verified against the oldest supported OS | CI | S | C9 |

**Tablet input is hand-written work, and it is on the critical path.** `research/05 §3.3` is
unambiguous: winit does not support tablets on macOS, and the fix is roughly 200 lines against
`objc2-app-kit`. Two details that the line count hides. First, macOS delivers tablet data
*attached to mouse events* as well as through dedicated tablet events, so the implementation
must handle both shapes or pressure will be missing during drags. Second, proximity events
(C3) are what let us switch to the eraser and what tell us the pen left the surface; without
them the tool state gets stuck. Treat C2+C3 as one deliverable.

**Sandboxing: we do not sandbox, and here is the honest reasoning.** The App Sandbox is
required only for Mac App Store distribution. Distributing outside the store with a Developer
ID plus notarisation gives users a clean Gatekeeper experience with none of the sandbox's
costs — chiefly security-scoped bookmarks for every remembered path, restricted access to the
user's font and asset directories, and friction with tablet drivers. Since Mac App Store
distribution is explicitly out of scope for 1.0, we ship **notarised, hardened-runtime,
unsandboxed**. The hardened runtime is still mandatory for notarisation, and the entitlements
list stays minimal: add `com.apple.security.cs.disable-library-validation` only if a
dependency forces it, and record why. If App Store distribution is ever wanted, the sandbox
work is a separate project, not a switch.

**Menus and shortcuts are more work than they look.** Xara's shortcut table
(`research/04 §4`, 142 lines including 24 nudge combinations) was designed for Windows. On
macOS, Ctrl becomes Cmd for the standard verbs, but function keys collide with system
features unless the user enables "use F1–F12 as standard function keys", and several Xara
bindings land on reserved combinations. C6 produces a per-platform default keymap table with
every conflict resolved and documented in the manual — not a blanket Ctrl→Cmd substitution.

### D. Cross-platform CI matrix

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| D1 | Build matrix: `x86_64-unknown-linux-gnu` (Ubuntu 22.04 container), `x86_64-pc-windows-msvc`, `x86_64-apple-darwin` + `aarch64-apple-darwin` | CI | M | A3 |
| D2 | Test matrix: headless suite everywhere; golden images via `vello_cpu` everywhere with **exact** match | CI | M | A4 |
| D3 | GPU parity nightly per platform: lavapipe on Linux, WARP on Windows, Metal on a macOS runner | CI | L | D2 |
| D4 | Artefact naming and retention: `Xarast-<version>-<os>-<arch>.<ext>`, plus checksums and the SBOM per platform | CI | S | D1 |
| D5 | Secrets: Azure Trusted Signing credentials, Apple `.p12` + app-specific password + team id, GPG key for Linux; least-privilege, documented rotation | CI | M | B13, C12 |
| D6 | Release job: one tag builds, tests, signs, notarises and publishes all three platforms; partial failure blocks the whole release | CI | L | D4, D5 |
| D7 | Per-platform `cargo deny`, `cargo about` and SBOM (dependency graphs differ by target) | CI | M | D1 |
| D8 | Build-time budget: full matrix under 45 minutes with caching (`sccache` or the Rust cache action) | CI | M | D1 |
| D9 | Phase 12 performance gates continue to run **only** on the Linux reference machine; Windows/macOS get informational (non-blocking) numbers | CI | M | D1 |

D9 is a deliberate limitation. A trustworthy performance gate needs a pinned, unshared
machine; we have one, on Linux. Publishing Windows and macOS numbers as informational tracks
regressions without pretending a hosted runner's variance is a gate. If a dedicated machine
for either platform appears later, promoting its numbers to gates is a small CI change.

D7 is easy to forget: the dependency graph is target-dependent (`objc2-*` on macOS,
`windows-sys` on Windows), so the licence audit and SBOM from Phase 12 must be produced
**per target**, not once.

### E. What will be second-class at first

This list is published in the release notes and in the manual. Understating it converts a
known limitation into a bug report and a bad review.

| Area | Linux | Windows | macOS | Note |
|---|---|---|---|---|
| Tablet pressure | First-class (Wayland `tablet_v2`) | Expected good (Ink); **Wintab fallback may be needed** | **New code, least tested** — C2/C3 is written for this phase and has the least hardware coverage |
| GPU backend maturity | Vulkan, heavily exercised | DX12, well supported | Metal via `wgpu`, less exercised by us |
| X11 (Linux) | Degraded: no pressure without the optional `octotablet` path | — | — | Already documented in `research/05 §3.4` |
| Accessibility | Orca-verified (Phase 12) | Narrator verification is new (B16) | VoiceOver verification is new (C14) | Expect gaps on all non-Linux at first release |
| Auto-update | zsync deltas | Check + manual download | Check + manual download | Delta updates are Linux-only for now |
| File associations | `.desktop` + MIME, well tested | MSI-declared, tested | `Info.plist` UTIs, tested | Portable ZIP on Windows registers nothing by design |
| Colour management | Not yet anywhere | — | — | Phase 15 |
| Printing | Not yet anywhere | — | — | Phase 15 |
| Multi-monitor mixed DPI | Tested | Tested (per-monitor v2) | Tested (backing scale) | The three models differ; expect per-platform quirks |
| Fractional scaling | Wayland-native | System-managed | System-managed | |
| ARM | aarch64 AppImage deferred to Phase 15 | Not supported | **Supported** (universal binary) | macOS is the only ARM target at this phase |

Stating that macOS tablet input is the least-tested part of the release is not a weakness; it
is what makes the first bug report about it actionable instead of surprising.

## Public API introduced

All of it inside `xarast-shell` except the keymap. No crate below the shell gains public API in
this phase — that is the point of workstream A.

```rust
// xarast-shell
pub trait TabletSource {
    fn poll(&mut self) -> Vec<StrokeSample>;
    fn capabilities(&self) -> TabletCaps;
}
pub struct StrokeSample {
    pub x: f64, pub y: f64,
    pub pressure: Option<f32>,
    pub tilt: Option<(f32, f32)>,
    pub twist: Option<f32>,
    pub tangential: Option<f32>,
    pub tool: ToolKind,          // Pen | Eraser | Mouse | Finger | Unknown
    pub timestamp: Instant,
    pub source: SourceId,
}
pub struct TabletCaps { pub pressure: bool, pub tilt: bool, pub twist: bool, pub proximity: bool }

pub enum Platform { Linux, Windows, MacOs }
pub fn platform() -> Platform;

pub enum GpuTier { Native, Portable, Compat, Software }   // DX12/Vulkan/Metal · Vulkan/GLES · GLES · vello_cpu
pub struct GpuSelection { pub tier: GpuTier, pub backend: wgpu::Backend, pub adapter_name: String }
pub fn select_gpu(prefs: &Prefs) -> GpuSelection;

pub trait Dialogs {
    fn open_file(&self, filters: &[FileFilter], start: Option<&Path>) -> Option<PathBuf>;
    fn save_file(&self, filters: &[FileFilter], suggested: &str) -> Option<PathBuf>;
    fn pick_folder(&self, start: Option<&Path>) -> Option<PathBuf>;
}
pub trait Clipboard {
    fn read(&self, flavours: &[Flavour]) -> Option<ClipboardPayload>;
    fn write(&self, payload: ClipboardPayload) -> Result<(), ClipboardError>;
}
pub enum Flavour { Text, Png, Svg, Dib, Pdf }

pub trait Appearance { fn colour_scheme(&self) -> ColourScheme; fn reduced_motion(&self) -> bool; }
pub trait Instance { fn acquire_or_forward(&self, files: &[PathBuf]) -> InstanceRole; }
pub enum InstanceRole { Primary, ForwardedToExisting }

// xarast-app
pub struct Keymap { /* … */ }
impl Keymap {
    pub fn platform_default(p: Platform) -> Self;
    pub fn conflicts(&self, p: Platform) -> Vec<Conflict>;   // system-reserved combinations
}
```

## Acceptance criteria

1. `cargo xtask audit-platform-boundary` reports **0** `cfg(target_os)` occurrences outside
   `xarast-shell`, `xarast-app::dirs`, `xarast-app::keymap` and `packaging/`.
2. `cargo nextest run --workspace` passes on all three OS runners; the golden-image suite runs
   through `vello_cpu` on each with **exact** pixel match against the same committed goldens.
3. `cargo build --release --target x86_64-pc-windows-msvc`, `--target x86_64-apple-darwin` and
   `--target aarch64-apple-darwin` all succeed from a clean checkout with `--locked`.
4. Windows artefacts verify: `signtool verify /pa /v Xarast-<v>-windows-x86_64.msi` and the
   same for the EXE exit 0; installing the MSI, opening a `.xar` by double-click, and
   uninstalling leaves no files under `%PROGRAMFILES%` and no `Xarast` keys under
   `HKCU\Software` (asserted by `packaging/windows/test-install.ps1`).
5. The portable ZIP build writes nothing outside its own directory during a full
   open/edit/save/export session (verified with Process Monitor filter rules scripted in
   `packaging/windows/test-portable.ps1`).
6. macOS artefacts verify: `codesign --verify --deep --strict --verbose=2 Xarast.app` exits 0,
   `spctl -a -vvv -t install Xarast.app` reports *accepted / Notarized Developer ID*, and
   `xcrun stapler validate Xarast.app` exits 0.
7. `lipo -info Xarast.app/Contents/MacOS/xarast` reports both `x86_64` and `arm64`, and the app
   launches on both an Apple Silicon and an Intel machine (or an Intel VM) running the minimum
   supported OS.
8. Tablet pressure verified on real hardware and recorded in a signed test log: at least one
   Wacom and one Huion on Windows, and at least one Wacom on macOS. For each: pressure varies
   continuously 0→1, tilt is reported if the device supports it, and the eraser end selects the
   eraser behaviour. A device that fails gets an issue and an entry in the second-class table.
9. Font enumeration: a freshly installed user font appears in the font gallery within one app
   restart on both platforms (`cargo nextest run -p xarast-text system_fonts` plus a manual
   confirmation for the install step).
10. Keymap: `cargo nextest run -p xarast-app keymap::conflicts` reports **0** unresolved
    conflicts with system-reserved shortcuts on each platform, and the generated keyboard
    reference has a column per platform.
11. Per-monitor DPI: dragging the window between a 100 % and a 200 % display re-renders crisply
    with no restart, on both Windows and macOS (manual, recorded, with screenshots).
12. Accessibility: the Phase 12 checklist is completed with Narrator on Windows and VoiceOver
    on macOS; every item is pass or has a filed issue.
13. Crash handling: `XARAST_FORCE_PANIC=render` produces a report in the platform's report
    directory, recovers the dirty document, and the next start offers safe mode — on all three
    platforms (`cargo nextest run -p xarast-app crash_recovery` on each runner).
14. The single release job produces, from one tag: a signed Linux AppImage with zsync, a signed
    Windows MSI and portable ZIP, and a notarised stapled macOS `.dmg` — plus checksums, a
    per-target SBOM and per-target `THIRD-PARTY-LICENSES.html`. A failure in any platform
    fails the release.
15. `cargo deny check` passes for each target triple independently.
16. CI full-matrix wall time **≤ 45 minutes** with warm caches.
17. Informational performance numbers are published for Windows and macOS for the standard
    scenario set, and the Linux gates from Phase 12 still pass unchanged.
18. The second-class table is published in the release notes and in the manual, and each entry
    that is a known defect has a tracking issue.

## Performance budgets

The Phase 12 budgets remain the gates, on Linux. For Windows and macOS this phase sets
**informational targets**; missing one is an issue, not a failed build. They exist so that a
platform that is twice as slow is noticed.

| Budget | Linux (gate) | Windows (target) | macOS (target) |
|---|---|---|---|
| Pan/zoom p95, 100k objects | ≤ 16 ms | ≤ 20 ms | ≤ 20 ms |
| Open a 5 MB `.xar` to first paint | ≤ 500 ms | ≤ 700 ms | ≤ 700 ms |
| Cold start to first frame | ≤ 400 ms | ≤ 700 ms | ≤ 700 ms |
| Peak RSS, 100k-object document | ≤ 1.5 GB | ≤ 1.7 GB | ≤ 1.7 GB |
| Installed size on disk | (AppImage ≤ 80 MB) | ≤ 200 MB | ≤ 250 MB (universal doubles the binary) |
| Stylus input-to-paint latency | measured | measured | measured |

The startup allowances are larger on Windows and macOS for concrete reasons: antivirus
interception of a freshly installed binary on Windows, and Gatekeeper's first-launch
verification on macOS. Both are first-launch effects; measure and publish cold-first-launch and
subsequent-launch separately, as Phase 12 C6 already does for the AppImage.

Stylus latency has no target yet because we have no Linux baseline to compare against. This
phase **measures** it on all three platforms with a high-speed capture or the platform's own
timestamps, publishes the numbers, and sets the target in Phase 15.

## Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| macOS tablet code (C2/C3) takes far longer than the ~200 lines estimated | **High** | High | Start it in week 1, not at the end; treat pressure-only as the minimum viable result and tilt/rotation as follow-ups; the second-class table absorbs the shortfall |
| Windows Ink misbehaves with common Wacom/Huion drivers | Medium | High | Test on real hardware before the release candidate; `wintab_lite` behind a preference (B4); if both test devices fail, promote Wintab to primary |
| Notarisation fails late for an entitlement or a signed-dylib reason | Medium | High | Run the full sign+notarise+staple path on a throwaway build in week 1 of the phase, long before the release job depends on it |
| Apple or Azure account procurement delays the phase | Medium | High | Both are prerequisites started before the phase opens; if either slips, everything except signing can still be completed and the artefacts held |
| Signing secrets leak or expire | Low | High | Least-privilege secrets, documented rotation, expiry calendar entries, and a release job that fails loudly rather than publishing unsigned |
| macOS CI runners are slow or scarce and the matrix blows the time budget | Medium | Medium | Cache aggressively; run the macOS GPU parity job nightly rather than per PR; universal builds only on release tags |
| `winit` 0.31 stays in beta and its API shifts | **High** (already flagged in `research/05 §2.6`) | Medium | The pinned `=0.31.0-beta.3` and the shell's trait boundary; budget one week per beta bump as the research already recommends |
| Platform work silently drifts into core crates and collides with Phase 13 | Medium | High | A1's lint plus the rule that Phase 14 PRs may not modify `xarast-doc`, `xarast-geom` or `xarast-render` except for portability fixes, which must say so in the commit message |
| Font enumeration differences produce different text layout per platform and break goldens | Medium | Medium | Golden tests use bundled test fonts loaded explicitly, never system fonts; confirm this is already true in Phase 9's suite and fix it here if not |
| Filesystem case-insensitivity on macOS breaks resource lookups in `.xarast` | Medium | Medium | A4 runs the format round-trip suite on macOS early; the container spec's paths are case-sensitive and must be treated as such |

## Test plan

**Automated on every PR, all three OSes**
- `cargo nextest run --workspace`, `cargo clippy -- -D warnings`, `cargo deny check` per target.
- Golden images through `vello_cpu`, exact match, same goldens on all platforms. Any platform
  divergence here is a real bug in the core, and finding those is one of this phase's main
  products.
- `.xar` and `.xarast` round trips; text layout snapshots using bundled fonts only.
- The platform-boundary lint (A5).

**Nightly**
- GPU parity per platform: lavapipe (Linux), WARP and a real GPU runner if available
  (Windows), Metal (macOS) — against the CPU oracle at the Phase 12 tolerances.
- Full packaging build for all three, including an unsigned dry run of the signing steps.

**Per release candidate, scripted**
- Windows: MSI install / upgrade-in-place / uninstall cleanliness (criterion 4); portable
  no-write check (criterion 5); double-click open for `.xar` and `.xarast`; SmartScreen
  behaviour recorded.
- macOS: `codesign`, `spctl`, `stapler` verification (criterion 6); `.dmg` mount and drag
  install; first-launch Gatekeeper flow recorded; launch on the minimum supported OS.
- Linux: the unchanged Phase 12 five-distro matrix.

**Manual hardware matrix, recorded in `docs/memory/packaging.md`**

| Platform | Hardware/config | What is checked |
|---|---|---|
| Windows 11 | NVIDIA discrete | DX12 tier, pan/zoom, stylus via Ink |
| Windows 11 | Intel integrated | Tier selection, performance numbers |
| Windows 10 | Any | Minimum supported OS launches and runs |
| Windows | Wacom Intuos | Pressure, tilt, eraser, proximity |
| Windows | Huion tablet | Same; the known-problem case |
| macOS 12 | Intel | Minimum supported OS, Metal, universal binary arch |
| macOS current | Apple Silicon | Metal, Retina, menu bar, shortcuts |
| macOS | Wacom | Pressure, tilt, rotation, eraser, proximity |
| All | Mixed-DPI dual monitor | Window drag between displays, crisp re-render |
| All | Screen reader | Narrator / VoiceOver / Orca on the Phase 12 checklist |

**Explicitly not tested and therefore not claimed**
- Windows on ARM, Mac App Store distribution, Windows Server, macOS older than the deployment
  target, and any tablet vendor outside the two tested on each platform. Each of these is
  listed as unsupported rather than untested-but-probably-fine.

## Memory note

On close, update:

- **`docs/memory/packaging.md`** — the platform-specific/not table from workstream A as the
  durable contract; per-platform build commands and toolchain versions; the signing procedures
  and secret names (never the secrets); the notarisation recipe including every entitlement and
  why it is there; MSI upgrade codes and the association registrations; the `.dmg` layout; the
  CI matrix shape and its wall time; the full manual hardware matrix with results and dates.
- **`docs/memory/ui.md`** — the per-platform keymap tables and every resolved conflict; menu
  bar differences; DPI/scaling behaviour per platform; Narrator and VoiceOver findings.
- **`docs/memory/render.md`** — GPU tier selection per platform, adapters tested, driver bugs
  found and their workarounds, and any platform divergence discovered by the shared golden
  suite.
- **`docs/memory/perf.md`** — the informational Windows and macOS numbers, the measured stylus
  latency on all three platforms, and the first-launch versus subsequent-launch startup split.
- **`docs/memory/text.md`** — font enumeration behaviour per platform, and the rule that golden
  tests use bundled fonts only.
- **`docs/memory/INDEX.md`** — if a `platform.md` note is created for the second-class table
  and its tracking issues, register it here.
