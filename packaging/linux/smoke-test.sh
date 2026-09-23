#!/usr/bin/env bash
#
# Boots a built Xarast AppImage on the machine it runs on and checks that it
# starts. It is meant to run inside a bare distribution container (Debian,
# Fedora, openSUSE, Arch/SteamOS) with no display, no FUSE and no packages
# installed beyond the base image, so it only uses bash, coreutils and the
# system dynamic loader.
#
#   smoke-test.sh <Xarast-ARCH.AppImage>
#
#   XARAST_CLI       optional path to an `xarast-cli` binary built alongside
#                    the image; when set, a headless CPU render is attempted
#   XARAST_SAMPLE    optional .xar file for that render
#
# Exits non-zero on the first failed check. docs/memory/packaging.md records
# what each check guards against.
#
set -euo pipefail

image="$(realpath "${1:?usage: smoke-test.sh <AppImage>}")"
# Containers have no FUSE; the runtime extracts to a temporary directory.
export APPIMAGE_EXTRACT_AND_RUN=1

pass() { printf 'PASS  %s\n' "$*"; }
fail() { printf 'FAIL  %s\n' "$*" >&2; exit 1; }

distro="unknown"
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  distro="$(. /etc/os-release && echo "${PRETTY_NAME:-$ID}")"
fi
glibc="$(getconf GNU_LIBC_VERSION 2>/dev/null || echo 'glibc ?')"
echo "== $distro ($glibc, $(uname -m))"

# 1. The image runs at all: runtime, extraction, AppRun, the binary's loader.
out="$("$image" --version)" || fail "--version exited $?"
[[ "$out" == xarast* ]] || fail "--version printed '$out'"
pass "--version: $out"

"$image" --help >/dev/null || fail "--help exited $?"
pass "--help"

out="$("$image" --selftest)" || fail "--selftest exited $?"
pass "--selftest: $(tail -n1 <<<"$out")"

# 2. With no compositor the window self-test must report it and exit 0: the
#    absence of a display server is a fact, not a fault.
unset WAYLAND_DISPLAY DISPLAY
out="$("$image" --selftest-window 2>&1)" || fail "--selftest-window exited $?: $out"
[[ "$out" == *"selftest-window: skipped"* ]] \
  || fail "--selftest-window did not report skipped: $out"
pass "--selftest-window: $(grep -m1 'selftest-window' <<<"$out")"

# 3. Every DT_NEEDED library resolves on this system, and none of the
#    compositor libraries is linked: libwayland-client must be dlopen'd so the
#    binary starts on hosts without it.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
(cd "$work" && "$image" --appimage-extract >/dev/null)
binary="$work/squashfs-root/usr/bin/xarast"
[[ -x "$binary" ]] || fail "no usr/bin/xarast in the image"

loader=""
for candidate in /lib64/ld-linux-x86-64.so.2 /lib/ld-linux-aarch64.so.1 \
                 /lib64/ld-linux-aarch64.so.1; do
  [[ -x "$candidate" ]] && { loader="$candidate"; break; }
done
[[ -n "$loader" ]] || fail "no dynamic loader found"
libs="$(LD_LIBRARY_PATH="$work/squashfs-root/usr/lib" "$loader" --list "$binary")" \
  || fail "the loader could not resolve the binary: $libs"
if grep -q 'not found' <<<"$libs"; then
  fail "missing libraries: $(grep 'not found' <<<"$libs" | tr -s ' \t' ' ')"
fi
if grep -Eq 'libwayland|libxkbcommon|libX11|libxcb|libvulkan|libGL|libEGL' <<<"$libs"; then
  fail "a compositor or GPU library is linked, not dlopen'd: $libs"
fi
pass "loader resolves $(grep -c '=>' <<<"$libs") libraries; no compositor/GPU library linked"

shopt -s nullglob globstar
bundled=("$work"/squashfs-root/usr/lib/**/*.so*)
pass "bundled libraries: ${#bundled[@]}"

# 4. Optional headless render through the CLI (not part of the image yet).
if [[ -n "${XARAST_CLI:-}" ]]; then
  "$XARAST_CLI" version >/dev/null || fail "xarast-cli version exited $?"
  if [[ -n "${XARAST_SAMPLE:-}" ]]; then
    "$XARAST_CLI" render "$XARAST_SAMPLE" -o "$work/render.png" --width 512 \
      || fail "xarast-cli render exited $?"
    [[ -s "$work/render.png" ]] || fail "xarast-cli render wrote nothing"
    pass "headless render: $(stat -c%s "$work/render.png") bytes of PNG"
  else
    pass "xarast-cli starts (no sample file, render skipped)"
  fi
fi

echo "== $distro: all checks passed"
