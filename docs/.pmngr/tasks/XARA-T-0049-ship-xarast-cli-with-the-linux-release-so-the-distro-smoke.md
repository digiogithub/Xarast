---
id: XARA-T-0049
type: task
title: Ship xarast-cli with the Linux release so the distro smoke job can render headless
status: backlog
parent: XARA-US-0066
author: mcp
labels: [packaging]
created: 2026-09-23T12:12:14Z
updated: 2026-09-23T12:25:03Z
---

## Description
The AppImage contains only `usr/bin/xarast`. `xarast-cli` (headless `render`, `smoke-open`) is not shipped, so the CI `distro-smoke` job cannot exercise a headless CPU render on each distribution. Locally (2026-09-23) a CLI built in the same ubuntu:22.04 container rendered `Designs/amurdove.xar` on Debian 12, Fedora 44, Tumbleweed, Leap 15.6 and Arch, so the binary itself is fine.

Options: bundle it in the AppDir and dispatch from AppRun on `argv[0]`/a subcommand; or publish it as a separate glibc-2.35 tarball artefact. The stripped CLI adds roughly 2-3 MB against ~72 MiB of headroom.

## Acceptance Criteria
- A decision recorded in `docs/memory/packaging.md`.
- The `distro-smoke` job passes `XARAST_CLI` to `packaging/linux/smoke-test.sh` and renders a small synthetic `.xar` fixture (the corpus must never be copied into the repository; a fixture must be authored clean-room).

## Notes
`smoke-test.sh` already supports `XARAST_CLI` and `XARAST_SAMPLE`.
