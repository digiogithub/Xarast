---
id: XARA-US-0019
type: story
title: As a Linux user, the AppImage is validated across distros
status: backlog
parent: XARA-EP-0018
author: mcp
labels: [packaging]
estimate: 3
created: 2026-09-23T09:40:52Z
updated: 2026-09-23T09:40:52Z
---

## Acceptance Criteria
- Distro smoke matrix (W0.4.8): boot on Debian 12, Fedora, openSUSE, SteamOS containers.
- `appstreamcli validate` in CI next to `desktop-file-validate`.
