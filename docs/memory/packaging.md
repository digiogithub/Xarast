# packaging

Memory note for the **packaging** subsystem (Linux AppImage first; Windows and
macOS in phase 14). Specification in
[`../phases/phase-00-foundations.md`](../phases/phase-00-foundations.md) §W0.4.

## Current state

- `packaging/linux/build-appimage.sh` produces a working AppImage.
- First real build: **3.9 MB**, against a 35 MiB phase 0 budget. It runs
  (`--version`, `--selftest`) and bundles **no libraries at all**.
- `cargo xtask icons` rasterises `assets/icons/xarast.svg` into the
  freedesktop theme sizes; CI fails if the committed PNGs are stale.
- CI builds x86_64 and aarch64, smoke-tests both, and attaches them to
  releases with zsync metadata.

## Decisions taken (and why)

1. **`linuxdeploy` + `linuxdeploy-plugin-appimage`.** MIT, a single static
   binary, and — crucially — we assemble the AppDir ourselves, so we know
   exactly what ships. `cargo-appimage` gives less control and is
   under-maintained; `appimage-builder` bundles aggressively by default,
   including exactly the libraries that must *not* be bundled.
2. **Bundle nothing that talks to the kernel, the GPU or the compositor.**
   `--exclude-library` covers libGL, libEGL, libgbm, libdrm, libvulkan,
   libwayland-*, libX11/libxcb, libxkbcommon, fontconfig, freetype, libstdc++
   and libgcc_s. A bundled Mesa will not match the host driver and a bundled
   Wayland client will not match the host compositor. This list is the most
   important part of the packaging.
3. **Build inside an explicit `ubuntu:22.04` container, not on the runner.**
   This is not caution, it is a measured requirement: the binary built on this
   development host requires **`GLIBC_2.39`**, which would fail to start on
   Debian 12, Ubuntu 22.04, RHEL 9 or SteamOS. The container pins the baseline
   at glibc **2.35**, and keeps it pinned when GitHub rotates runner images.
4. **Never link musl.** It breaks `dlopen` of the system GL and Vulkan drivers,
   which is the one thing an AppImage must not break.
5. **`APPIMAGE_EXTRACT_AND_RUN=1` everywhere**, local and CI alike, so the two
   paths are the same path. Containers have no FUSE.
6. **`RUSTFLAGS=-C target-cpu=x86-64-v2` on x86_64 only.** SSE4.2 and POPCNT
   have been safe since 2009 and are worth real throughput in the rasteriser.
   aarch64 keeps the default target CPU.
7. **`panic = "unwind"`, not `abort`.** `cargo-fuzz` needs to catch a panic as
   a finding rather than losing the process, and the `.xar` importer is fuzzed
   from day one.

## Invariants that must not be broken

1. The Wayland/X11 `app_id`, the desktop entry basename and its
   `StartupWMClass` are all `xarast`. A test in `xarast-shell` enforces it;
   breaking it gives a generic icon and mis-grouped windows, which is invisible
   until someone installs the package.
2. The AppImage must start on glibc 2.35. Verify with `objdump -T` in CI, not
   by hope.
3. No library from the exclusion list may appear in `usr/lib` of the AppDir.
4. Committed icon PNGs must be reproducible from the SVG by `cargo xtask icons`.

## Dead ends (do not retry)

- Building the release AppImage on the GitHub runner image directly: produces a
  GLIBC_2.39 binary. Measured, not theoretical.
- Relying on FUSE being present anywhere in CI.

## Open TODOs

- [ ] Distro smoke matrix (W0.4.8): actually boot the image on Debian 12,
      Fedora, openSUSE and SteamOS containers.
- [ ] AppStream validation in CI — `appstreamcli validate` is not yet wired up;
      only `desktop-file-validate` runs.
- [ ] GPG signing of the image and the zsync file (phase 12).
- [ ] Re-measure the size budget once wgpu, egui and the font stack are linked
      in; 3.9 MB will not survive phase 5.
